use std::{
    collections::{BTreeMap, HashMap},
    iter::empty,
    panic,
    sync::Arc,
};

use crate::thir::inference::constraints::InferenceConstraintKind;
use crate::{
    Db,
    common::location::Span,
    hir::{
        self, HirBody, HirExpr, HirId, HirPattern, HirPatternDesc, HirStmt, HirStmtKind, LocalId,
        LocalInfo, PartialTypeRef, hir_body, owning_module,
    },
    name_resolve::type_expr::{enum_item, get_templates_of_fun, templates_of_enum},
    parse_tree::{
        top_level::{AstEnumVariantKind, AstTemplateArg},
        type_expr::AstAnyTypeExprDesc,
    },
    ril::{self, FunctionId, InternedFunctionId, Package, TypeDefId, TypeRef},
    thir::inference::{
        InferTy, InferenceCtx, UnificationError,
        implicit::{AsAstImplCtx, ImplicitContext},
        var::InferVar,
    },
};

pub mod inference;

#[derive(Clone)]
struct InferCallInfos {
    expr_id: ExprId,
    callee: FunctionId,
    substitution: Box<[InferTy]>,
    variadic: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CallInfos {
    pub expr_id: ExprId,
    pub callee: FunctionId,
    pub substitution: Vec<TypeRef>,
    pub variadic: bool,
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub enum DiagnosticKind {
    UniError {
        err: UnificationError,
        message: String,
    },
    BadRetType(TyRef),
    BadAssignement(UnificationError),
}

#[derive(Clone)]
#[allow(dead_code)]
struct TyCtx<'db> {
    db: &'db dyn Db,
    function: FunctionId,
    locals: &'db [LocalInfo],
    params: &'db [LocalId],
    packages: Arc<[Package<'db>]>,

    templates: Arc<[AstTemplateArg]>,

    inf_ctx: InferenceCtx<'db>,

    calls: HashMap<ExprId, InferCallInfos>,
    exprs: HashMap<ExprId, TyRef>,

    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExprId(HirId);

// #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
// pub struct FieldId(usize);

// #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
// pub struct PatternId(HirId);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TyRef {
    Inf(InferTy),
    Error,
}

impl<'db> TyCtx<'db> {
    pub fn new(
        db: &'db dyn Db,
        function: FunctionId,
        locals: &'db [LocalInfo],
        params: &'db [LocalId],
        packages: Arc<[Package<'db>]>,
    ) -> Self {
        let templates = get_templates_of_fun(db, function.interned());
        let local_ids = locals.iter().map(|local| local.id).collect::<Box<[_]>>();
        let inf_ctx = InferenceCtx::new(db, &local_ids, function, params, packages.clone());

        Self {
            db,
            function,
            locals,
            params,
            templates,
            packages,
            inf_ctx,
            calls: HashMap::new(),
            exprs: HashMap::new(),
            diagnostics: Vec::new(),
        }
    }

    fn concretize_infos(&mut self, infos: InferCallInfos) -> CallInfos {
        CallInfos {
            expr_id: infos.expr_id,
            callee: infos.callee,
            substitution: infos
                .substitution
                .into_iter()
                .map(|ty| self.inf_ctx.solve(ty).unwrap_or(TypeRef::Error))
                .collect(),
            variadic: infos.variadic,
        }
    }

    fn finalize(mut self) -> TypeCheckResults<'db> {
        if let Err((constraint, err)) = self.inf_ctx.solve_constraints() {
            println!(
                "Error solving constraint {}",
                constraint.kind.display(self.db)
            );
            println!("    Reason: {}", err.display(self.db));
            panic!()
        }

        let unsolveds = self.inf_ctx.get_current_constraints();
        if !unsolveds.is_empty() {
            println!("-- Constraints not solved ----------------------");
            for c in unsolveds {
                println!("Constraint not solved:\n    {}", c.kind.display(self.db))
            }
            println!("-- \\Constraints not solved ---------------------");
        }

        let drain = self.exprs.drain().collect::<Box<[_]>>();
        let node_types = drain
            .into_iter()
            .map(|(id, infer_ty)| {
                (
                    id,
                    match infer_ty {
                        TyRef::Inf(infer_ty) => {
                            self.inf_ctx.solve(infer_ty).unwrap_or(TypeRef::Error)
                        }
                        TyRef::Error => TypeRef::Error,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let call_infos = self
            .calls
            .drain()
            .collect::<Box<[_]>>()
            .into_iter()
            .map(|(id, infos)| (id, self.concretize_infos(infos)))
            .collect();
        let diagnostics = self.diagnostics.drain(..).collect::<Vec<_>>();
        TypeCheckResults::new(self.db, node_types, call_infos, diagnostics)
    }

    fn type_check_expr(&mut self, expr: &'db HirExpr) -> (TyRef, Option<UnificationError>) {
        let (ty, err) = match self.inf_ctx.infer_expr(expr) {
            Ok(infer_ty) => (TyRef::Inf(infer_ty), None),
            Err(err) => (TyRef::Error, Some(err)),
        };
        self.exprs.insert(ExprId(expr.id), ty.clone());
        (ty, err)
    }

    fn get_ret_ty(&self) -> InferTy {
        let ret = self.function.ret_ty(self.db);
        self.inf_ctx
            .allocate_type_ref(&ret, &self.inf_ctx.implicit_ctx())
    }

    fn push_regular_diagnostic(&mut self, err: UnificationError, span: Span) {
        self.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::UniError {
                err,
                message: String::new(),
            },
            span,
        })
    }

    fn typeof_pattern(
        &mut self,
        pattern: &HirPattern,
        loc_inners: &HashMap<LocalId, InferVar>,
    ) -> InferTy {
        match &pattern.data {
            HirPatternDesc::Bind { id, .. } => {
                // TODO: is this right ?
                InferTy::Var(*loc_inners.get(id).unwrap())
            }
            HirPatternDesc::Any => InferTy::Var(self.inf_ctx.fresh_var()),
            HirPatternDesc::Tuple(hir_patterns) => todo!(),
            HirPatternDesc::DestructureBinding { resolution, fields } => todo!(),
            HirPatternDesc::Constructor {
                resolution,
                name,
                fields,
            } => {
                let item = enum_item(self.db, resolution.interned());
                let infer_template_vars: Arc<[InferVar]> =
                    templates_of_enum(self.db, resolution.interned())
                        .iter()
                        .map(|_| self.inf_ctx.fresh_var())
                        .collect();
                let infer_templates: Arc<[InferTy]> = infer_template_vars
                    .iter()
                    .copied()
                    .map(InferTy::Var)
                    .collect();
                let ctx = ImplicitContext::new(
                    self.db,
                    ril::ScopeOwnerId::Module(resolution.parent(self.db)),
                    item.template_args.iter().cloned().collect(),
                    infer_templates.clone(),
                    None, // TODO: Is this right ?
                )
                .unwrap();
                let t_ref = InferTy::Adt {
                    def: TypeDefId::Enum(*resolution),
                    fields: infer_templates.iter().cloned().collect(),
                };
                let variant = item
                    .variants
                    .iter()
                    .find(|variant| variant.name == *name)
                    .expect("Variants of enum should already have been checked");
                match (fields, &variant.kind) {
                    (hir::HirPatternConstructorArgs::None, AstEnumVariantKind::Unit) => (),
                    (
                        hir::HirPatternConstructorArgs::StructFields(hir_fields),
                        AstEnumVariantKind::StructLike(ast_fields),
                    ) => todo!(),
                    (
                        hir::HirPatternConstructorArgs::TupleFields(hir_patterns),
                        AstEnumVariantKind::TupleLike(ast_patterns),
                    ) => {
                        assert!(hir_patterns.len() == ast_patterns.len());
                        for (hir_pattern, ast_pattern) in
                            hir_patterns.iter().zip(ast_patterns.iter())
                        {
                            let pat_ty = self.typeof_pattern(hir_pattern, loc_inners);
                            let ast_ty = self
                                .inf_ctx
                                .allocate_ast_type_expr(&ast_pattern.data, &ctx)
                                .unwrap();
                            self.inf_ctx
                                .emit_constraint(InferenceConstraintKind::Unify {
                                    a: pat_ty,
                                    b: ast_ty,
                                });
                        }
                    }
                    _ => unreachable!("Mismatch between AST variant kind decl and case"),
                }
                t_ref
            }
        }
    }

    fn type_check_stmt(&mut self, stmt: &'db HirStmt) {
        match &stmt.kind {
            HirStmtKind::Let {
                pattern,
                ty_annotation,
                init,
                ..
            } => {
                let (init_ty, err) = self.type_check_expr(init);
                if let Some(err) = err {
                    self.push_regular_diagnostic(err, init.span.clone());
                }
                match self.inf_ctx.infer_pattern(pattern, None) {
                    Ok(pattern_ty) => match init_ty {
                        TyRef::Inf(infer_ty) => {
                            if let Err(err) = self.inf_ctx.unify(infer_ty.clone(), pattern_ty) {
                                self.push_regular_diagnostic(err, stmt.span.clone());
                            } else if let Some(annotation) = ty_annotation
                                && let Some(annotation) = annotation.as_known()
                                && let Some(annotated) = self.inf_ctx.allocate_ast_type_expr(
                                    &annotation.data,
                                    self.inf_ctx.implicit_ctx().as_ref(),
                                    // owning_module(self.db, self.function.parent(self.db)),
                                    // &self.templates,
                                    // &self.inf_ctx.templates(),
                                    // self.inf_ctx.zelf(),
                                )
                                && let Err(err) = self.inf_ctx.unify(infer_ty, annotated)
                            {
                                self.push_regular_diagnostic(err, annotation.span.clone());
                            }
                        }
                        TyRef::Error => (),
                    },
                    Err(err) => self.push_regular_diagnostic(err, pattern.span.clone()),
                }
            }
            HirStmtKind::Match {
                scrutinee,
                branches,
            } => {
                let (typeof_scrut, err) = self.type_check_expr(scrutinee);
                if let Some(err) = err {
                    self.push_regular_diagnostic(err, scrutinee.span.clone());
                }
                let typeof_scrut = match typeof_scrut {
                    TyRef::Inf(infer_ty) => infer_ty,
                    TyRef::Error => {
                        println!(
                            "TODO: handle this but I don't want to make it terminate the program"
                        );
                        return;
                    }
                };
                let typeof_scrut_var = self.inf_ctx.fresh_var();
                self.inf_ctx
                    .emit_constraint(InferenceConstraintKind::Unify {
                        a: typeof_scrut,
                        b: InferTy::Var(typeof_scrut_var),
                    });
                for branch in branches {
                    let mut loc_inners = HashMap::new();
                    for local in &branch.locals {
                        let typeof_local = self.inf_ctx.local_var(*local);
                        let inner_var = self.inf_ctx.fresh_var();
                        self.inf_ctx
                            .emit_constraint(InferenceConstraintKind::BindsLike {
                                ty: typeof_local,
                                inner: InferTy::Var(inner_var),
                                like: typeof_scrut_var,
                            });
                        loc_inners.insert(*local, inner_var);
                    }

                    // Now, each local has a binding mode
                    // We may want to try and have a substitution map
                    // for the inner types and check the patterns
                    // against them instead of the raw local vars
                    let typeof_pattern = self.typeof_pattern(&branch.pattern, &loc_inners);
                    self.inf_ctx
                        .emit_constraint(InferenceConstraintKind::Unify {
                            a: typeof_pattern,
                            b: InferTy::Var(typeof_scrut_var),
                        });
                }
                // todo!()
            }
            HirStmtKind::Assign { lhs, rhs } => {
                let (rhs_ty, rhs_err) = self.type_check_expr(rhs);
                if let Some(rhs_err) = rhs_err {
                    self.push_regular_diagnostic(rhs_err, rhs.span.clone());
                }
                match self.inf_ctx.infer_place(lhs) {
                    Ok(lhs_ty) => match rhs_ty {
                        TyRef::Inf(rhs_ty) => {
                            if let Err(err) = self.inf_ctx.unify(lhs_ty, rhs_ty) {
                                self.diagnostics.push(Diagnostic {
                                    kind: DiagnosticKind::BadAssignement(err),
                                    span: stmt.span.clone(),
                                })
                            }
                        }
                        TyRef::Error => (),
                    },
                    Err(err) => self.push_regular_diagnostic(err, rhs.span.clone()),
                }
            }
            HirStmtKind::Expr(hir_expr) => {
                let (_, err) = self.type_check_expr(hir_expr);
                if let Some(err) = err {
                    self.diagnostics.push(Diagnostic {
                        kind: DiagnosticKind::UniError {
                            err,
                            message: String::new(),
                        },
                        span: stmt.span.clone(),
                    });
                }
            }
            HirStmtKind::Return(hir_expr) => {
                let ret_ty = self.get_ret_ty();
                if let Some(expr) = &hir_expr {
                    let (ty, err) = self.type_check_expr(expr);
                    if let Some(err) = err {
                        self.diagnostics.push(Diagnostic {
                            kind: DiagnosticKind::UniError {
                                err,
                                message: String::new(),
                            },
                            span: stmt.span.clone(),
                        })
                    };

                    match ty {
                        TyRef::Inf(infer_ty) => {
                            if let Err(err) = self.inf_ctx.unify(ret_ty, infer_ty.clone()) {
                                self.push_regular_diagnostic(err, stmt.span.clone());
                                self.diagnostics.push(Diagnostic {
                                    kind: DiagnosticKind::BadRetType(TyRef::Inf(infer_ty)),
                                    span: stmt.span.clone(),
                                });
                            }
                        }
                        TyRef::Error => {
                            self.diagnostics.push(Diagnostic {
                                kind: DiagnosticKind::BadRetType(TyRef::Error),
                                span: stmt.span.clone(),
                            });
                        }
                    }
                } else {
                    let void_ty = self.inf_ctx.void_ty();
                    if ret_ty != void_ty {
                        self.diagnostics.push(Diagnostic {
                            kind: DiagnosticKind::BadRetType(TyRef::Inf(void_ty)),
                            span: stmt.span.clone(),
                        })
                    }
                }
            }
            HirStmtKind::If { .. } => todo!(),
            HirStmtKind::While { cond, body } => match self.inf_ctx.infer_expr(cond) {
                Ok(ty) => {
                    if let Err(err) = self.inf_ctx.unify(ty, self.inf_ctx.bool_ty()) {
                        self.push_regular_diagnostic(err, cond.span.clone());
                        return;
                    }
                    self.type_check_stmt(body);
                }
                Err(err) => {
                    self.push_regular_diagnostic(err, cond.span.clone());
                }
            },
            HirStmtKind::Block(stmts) => stmts.iter().for_each(|stmt| self.type_check_stmt(stmt)),
            HirStmtKind::Defer(stmt) => self.type_check_stmt(stmt),
            HirStmtKind::Break => todo!(),
        }
    }

    fn type_check(mut self, stmts: &'db [HirStmt]) -> TypeCheckResults<'db> {
        stmts.iter().for_each(|stmt| self.type_check_stmt(stmt));
        self.finalize()
    }
}

#[salsa::tracked]
pub struct TypeCheckResults<'db> {
    pub node_types: BTreeMap<ExprId, TypeRef>,
    pub call_infos: BTreeMap<ExprId, CallInfos>,
    pub diagnostics: Vec<Diagnostic>,
}

fn type_check_hir<'db>(
    db: &'db dyn Db,
    hir: HirBody<'db>,
    packages: Arc<[Package<'db>]>,
) -> TypeCheckResults<'db> {
    TyCtx::new(db, hir.owner(db), hir.locals(db), hir.params(db), packages)
        .type_check(hir.stmts(db))
}

#[salsa::tracked]
pub fn type_check_function<'db>(
    db: &'db dyn Db,
    function: InternedFunctionId<'db>,
    packages: Box<[Package<'db>]>,
) -> Option<TypeCheckResults<'db>> {
    hir_body(db, function).map(|hir| type_check_hir(db, hir, packages.into()))
}
