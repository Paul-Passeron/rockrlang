#![allow(dead_code)]

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use crate::{
    Db,
    common::location::Span,
    hir::{
        HirBody, HirExpr, HirId, HirPattern, HirStmt, HirStmtKind, LocalId, LocalInfo, hir_body,
        owning_module,
    },
    name_resolve::type_expr::get_templates_of_fun,
    parse_tree::{top_level::AstTemplateArg, type_expr::AstAnyTypeExprDesc},
    ril::{FunctionId, InternedFunctionId, Package, TypeRef},
    thir::inference::{InferTy, InferenceCtx, UnificationError},
};

pub mod inference;
pub mod methods;

#[derive(Clone)]
struct InferCallInfos {
    expr_id: ExprId,
    callee: FunctionId,
    substitution: Vec<InferTy>,
    variadic: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CallInfos {
    pub expr_id: ExprId,
    pub callee: FunctionId,
    pub substitution: Vec<TypeRef>,
    pub variadic: bool,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    kind: DiagnosticKind,
    span: Span,
}

#[derive(Debug, Clone)]
pub enum DiagnosticKind {
    UniError {
        err: UnificationError,
        message: String,
    },
    BadRetType(TyRef),
}

impl InferCallInfos {
    fn new(
        expr_id: ExprId,
        callee: FunctionId,
        substitution: Vec<InferTy>,
        variadic: bool,
    ) -> Self {
        Self {
            expr_id,
            callee,
            substitution,
            variadic,
        }
    }
}

#[derive(Clone)]
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExprId(HirId);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldId(usize);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatternId(HirId);

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
        let inf_ctx = InferenceCtx::new(db, &local_ids, templates.clone(), packages.clone());
        let mut this = Self {
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
        };

        let infer_templates = this.inf_ctx.templates();
        for param in params {
            let local = locals.iter().find(|x| &x.id == param).unwrap();
            if let Some(annotation) = local.ty_annotation.as_ref()
                && let AstAnyTypeExprDesc::Known(desc) = &annotation.data
            {
                let param_ty = this
                    .inf_ctx
                    .allocate_ast_type_expr(
                        desc,
                        owning_module(this.db, this.function.parent(this.db)),
                        this.templates.as_ref(),
                        infer_templates.as_ref(),
                    )
                    .unwrap();
                let local_ty = this.inf_ctx.infer_local(local.id);
                this.inf_ctx.unify(param_ty, local_ty).unwrap();
            }
        }
        this
    }

    fn concretize_infos(&self, infos: InferCallInfos) -> CallInfos {
        CallInfos {
            expr_id: infos.expr_id,
            callee: infos.callee,
            substitution: infos.substitution.into_iter().map(|_| todo!()).collect(),
            variadic: infos.variadic,
        }
    }

    fn finalize(mut self) -> TypeCheckResults<'db> {
        self.inf_ctx.solve_constraints().unwrap();
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

        TypeCheckResults::new(self.db, node_types, call_infos)
    }

    fn type_check_expr(&mut self, expr: &'db HirExpr) -> TyRef {
        let ty = self
            .inf_ctx
            .infer_expr(expr)
            .map_or(TyRef::Error, TyRef::Inf);
        self.exprs.insert(ExprId(expr.id), ty.clone());
        ty
    }

    fn get_ret_ty(&self) -> TyRef {
        let ret = self.function.ret_ty(self.db);
        TyRef::Inf(
            self.inf_ctx
                .allocate_type_ref(&ret, self.inf_ctx.templates().as_ref()),
        )
    }

    fn type_check_patt(&mut self, _pattern: &'db HirPattern) -> InferTy {
        todo!()
    }

    fn type_check_stmt(&mut self, stmt: &'db HirStmt) {
        match &stmt.kind {
            HirStmtKind::Let { .. } => todo!(),
            HirStmtKind::Match { .. } => todo!(),
            HirStmtKind::Assign { .. } => todo!(),
            HirStmtKind::Expr(hir_expr) => {
                if let Err(err) = self.inf_ctx.infer_expr(hir_expr) {
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
                let ty = hir_expr
                    .as_ref()
                    .map(|expr| {
                        self.inf_ctx
                            .infer_expr(expr)
                            .map_err(|err| {
                                self.diagnostics.push(Diagnostic {
                                    kind: DiagnosticKind::UniError {
                                        err,
                                        message: String::new(),
                                    },
                                    span: stmt.span.clone(),
                                })
                            })
                            .map_or(TyRef::Error, TyRef::Inf)
                    })
                    .unwrap_or(TyRef::Inf(self.inf_ctx.void_ty()));

                if self.get_ret_ty() != ty {
                    self.diagnostics.push(Diagnostic {
                        kind: DiagnosticKind::BadRetType(ty),
                        span: stmt.span.clone(),
                    });
                }
            }
            HirStmtKind::If { .. } => todo!(),
            HirStmtKind::While { .. } => todo!(),
            HirStmtKind::Block(_) => todo!(),
            HirStmtKind::Defer(_) => todo!(),
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
