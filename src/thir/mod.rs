/* Rockr programming language
Copyright (C) 2026  NoRezap

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

use std::collections::HashMap;
use std::{collections::BTreeMap, panic, sync::Arc};

use crate::compiler::diagnostic::Diagnostic;
use crate::hir::function_ast;

use crate::thir::inference::constraints::InferenceConstraintKind;
use crate::{
    Db,
    hir::{
        self, HirBody, HirExpr, HirId, HirPattern, HirPatternDesc, HirStmt, HirStmtKind, LocalId,
        LocalInfo, hir_body,
    },
    name_resolve::type_expr::{enum_item, get_templates_of_fun, templates_of_enum},
    parse_tree::top_level::{AstEnumVariantKind, AstTemplateArg},
    ril::{self, FunctionId, InternedFunctionId, Package, TypeDefId, TypeRef},
    thir::inference::{
        InferTy, InferenceCtx, UnificationError, implicit::ImplicitContext, var::InferVar,
    },
};

pub mod inference;

#[derive(Clone)]
pub(super) struct InferCallInfos {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExprId(HirId);

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
        zelf: Option<LocalId>,
        packages: Arc<[Package<'db>]>,
    ) -> Self {
        let templates = get_templates_of_fun(db, function.interned());
        let local_ids = locals.iter().map(|local| local.id).collect::<Box<[_]>>();
        let inf_ctx = InferenceCtx::new(db, &local_ids, function, zelf, params, packages.clone());

        Self {
            db,
            function,
            locals,
            params,
            templates,
            packages,
            inf_ctx,
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
        if let Err((_, err)) = self.inf_ctx.solve_constraints() {
            let span = function_ast(self.db, self.function.interned())
                .inner(self.db)
                .get_span();
            self.inf_ctx.diagnostics.push_regular_diagnostic(err, span);
        }

        // Temporary
        let unsolveds = self.inf_ctx.unsolved_constraints();
        for unsolved in unsolveds {
            let span = function_ast(self.db, self.function.interned())
                .inner(self.db)
                .get_span();
            let txt = format!("<UNSOLVED> {}", unsolved.kind.display(self.db));
            self.inf_ctx
                .diagnostics
                .push_regular_diagnostic_with_message(txt, span);
        }

        let drain = std::mem::take(&mut self.inf_ctx.inferred_exprs)
            .into_iter()
            .collect::<Box<[_]>>();
        let node_types = drain
            .into_iter()
            .map(|(id, infer_ty)| (id, self.inf_ctx.solve(infer_ty).unwrap_or(TypeRef::Unknown)))
            .collect::<BTreeMap<_, _>>();

        let call_infos = self
            .inf_ctx
            .drain_call_infos()
            .into_iter()
            .map(|(id, infos)| (id, self.concretize_infos(infos)))
            .collect();

        let diagnostics = self.inf_ctx.diagnostics.drain();

        let locals = self
            .locals
            .iter()
            .map(|local| {
                (
                    local.id,
                    self.inf_ctx
                        .solve(InferTy::Var(self.inf_ctx.local_var(local.id))),
                )
            })
            .collect();

        TypeCheckResults::new(self.db, node_types, call_infos, diagnostics, locals)
    }

    fn type_check_expr(&mut self, expr: &'db HirExpr) -> (TyRef, Option<UnificationError>) {
        let (ty, err) = match self.inf_ctx.infer_expr(expr) {
            Ok(infer_ty) => (TyRef::Inf(infer_ty), None),
            Err(err) => (TyRef::Error, Some(err)),
        };
        (ty, err)
    }

    fn get_ret_ty(&mut self) -> InferTy {
        let ret = self.function.ret_ty(self.db);
        self.inf_ctx
            .allocate_type_ref(&ret, &self.inf_ctx.implicit_ctx())
    }

    fn typeof_pattern(
        &mut self,
        pattern: &HirPattern,
        loc_inners: &HashMap<LocalId, InferVar>,
    ) -> InferTy {
        match &pattern.data {
            HirPatternDesc::Bind { id, .. } => {
                // TODO: is this right ?
                InferTy::Var(
                    *loc_inners
                        .get(id)
                        .expect("Internal error: local referenced in hir but not found"),
                )
            }
            HirPatternDesc::Any => InferTy::Var(self.inf_ctx.fresh_var()),
            HirPatternDesc::Tuple(_hir_patterns) => todo!(),
            HirPatternDesc::DestructureBinding { .. } => todo!(),
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
                let Ok(ctx) = ImplicitContext::new(
                    self.db,
                    ril::ScopeOwnerId::Module(resolution.parent(self.db)),
                    item.template_args.iter().cloned().collect(),
                    infer_templates.clone(),
                    None, // TODO: Is this right ?
                ) else {
                    todo!("Diagnostics")
                };
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
                        hir::HirPatternConstructorArgs::StructFields(_hir_fields),
                        AstEnumVariantKind::StructLike(_ast_fields),
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
                            let Some(ast_ty) =
                                self.inf_ctx.allocate_ast_type_expr(&ast_pattern.data, &ctx)
                            else {
                                self.inf_ctx.diagnostics.push_regular_diagnostic_with_message(
                                    format!(
                                        "Could not allocate ast_type_expr for some reason at {}:{}",
                                        file!(),
                                        line!()
                                    ),
                                    pattern.span.clone(),
                                );
                                return InferTy::Var(self.inf_ctx.fresh_var());
                            };
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
            HirPatternDesc::IntLit(_) => InferTy::Var(self.inf_ctx.emit_intlike_constraint()),
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
                    self.inf_ctx
                        .diagnostics
                        .push_regular_diagnostic(err, init.span.clone());
                }
                match self.inf_ctx.infer_pattern(pattern, None) {
                    Ok(pattern_ty) => match init_ty {
                        TyRef::Inf(infer_ty) => {
                            if let Err(err) = self.inf_ctx.unify(infer_ty.clone(), pattern_ty) {
                                self.inf_ctx
                                    .diagnostics
                                    .push_regular_diagnostic(err, stmt.span.clone());
                            } else if let Some(annotation) = ty_annotation
                                && let Some(annotation) = annotation.as_known()
                                && let Some(annotated) = self.inf_ctx.allocate_ast_type_expr(
                                    &annotation.data,
                                    self.inf_ctx.implicit_ctx().as_ref(),
                                )
                                && let Err(err) = self.inf_ctx.unify(infer_ty, annotated)
                            {
                                self.inf_ctx
                                    .diagnostics
                                    .push_regular_diagnostic(err, annotation.span.clone());
                            }
                        }
                        TyRef::Error => (),
                    },
                    Err(err) => self
                        .inf_ctx
                        .diagnostics
                        .push_regular_diagnostic(err, pattern.span.clone()),
                }
            }
            HirStmtKind::Match {
                scrutinee,
                branches,
            } => {
                let (typeof_scrut, err) = self.type_check_expr(scrutinee);
                if let Some(err) = err {
                    self.inf_ctx
                        .diagnostics
                        .push_regular_diagnostic(err, scrutinee.span.clone());
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
                        a: typeof_scrut.clone(),
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

                    let typeof_pattern = self.typeof_pattern(&branch.pattern, &loc_inners);

                    self.inf_ctx
                        .emit_constraint(InferenceConstraintKind::IsInner {
                            inner: typeof_pattern,
                            ref_ty: typeof_scrut.clone(),
                        });
                }
            }
            HirStmtKind::Assign { lhs, rhs } => {
                let (rhs_ty, rhs_err) = self.type_check_expr(rhs);
                if let Some(rhs_err) = rhs_err {
                    self.inf_ctx
                        .diagnostics
                        .push_regular_diagnostic(rhs_err, rhs.span.clone());
                }
                match self.inf_ctx.infer_place(lhs) {
                    Ok(lhs_ty) => match rhs_ty {
                        TyRef::Inf(rhs_ty) => {
                            if let Err(err) = self.inf_ctx.unify(lhs_ty.clone(), rhs_ty.clone()) {
                                let lstr = self.inf_ctx.find(&lhs_ty).to_string(self.db);
                                let rstr = self.inf_ctx.find(&rhs_ty).to_string(self.db);
                                self.inf_ctx
                                    .diagnostics
                                    .push_regular_diagnostic_with_message_and_primary(
                                        format!("cannot assign {lstr} to {rstr}"),
                                        Some(err.display(self.db).to_string()),
                                        stmt.span.clone(),
                                    );
                            }
                        }
                        TyRef::Error => (),
                    },
                    Err(err) => self
                        .inf_ctx
                        .diagnostics
                        .push_regular_diagnostic(err, rhs.span.clone()),
                }
            }
            HirStmtKind::Expr(hir_expr) => {
                let (_, err) = self.type_check_expr(hir_expr);
                if let Some(err) = err {
                    self.inf_ctx
                        .diagnostics
                        .push_regular_diagnostic(err, stmt.span.clone());
                }
            }
            HirStmtKind::Return(hir_expr) => {
                let ret_ty = self.get_ret_ty();
                if let Some(expr) = &hir_expr {
                    let (ty, err) = self.type_check_expr(expr);
                    if let Some(err) = err {
                        self.inf_ctx
                            .diagnostics
                            .push_regular_diagnostic(err, stmt.span.clone());
                    };

                    match ty {
                        TyRef::Inf(infer_ty) => {
                            if let Err(err) = self.inf_ctx.unify(ret_ty.clone(), infer_ty.clone()) {
                                let fmt = format!(
                                    "Cannot return {} form a function expected to return {}",
                                    self.inf_ctx.find(&infer_ty).to_string(self.db),
                                    self.inf_ctx.find(&ret_ty).to_string(self.db)
                                );
                                self.inf_ctx
                                    .diagnostics
                                    .push_regular_diagnostic_with_message_and_primary(
                                        fmt,
                                        Some(err.display(self.db).to_string()),
                                        stmt.span.clone(),
                                    );
                            }
                        }
                        TyRef::Error => {
                            self.inf_ctx.diagnostics.push_regular_diagnostic_with_message(
                                format!(
                                    "Cannot return error type form a function expected to return {}",
                                    ret_ty.to_string(self.db)
                                ),
                                stmt.span.clone(),
                            );
                        }
                    }
                } else {
                    let void_ty = self.inf_ctx.void_ty();
                    if ret_ty != void_ty {
                        self.inf_ctx.diagnostics.push_regular_diagnostic_with_message(
                            format!(
                                "Cannot have an empty return from a function expected to return {}",
                                ret_ty.to_string(self.db)
                            ),
                            stmt.span.clone(),
                        );
                    }
                }
            }
            HirStmtKind::If { cond, then, else_ } => {
                match self.inf_ctx.infer_expr(cond) {
                    Ok(ty) => match self.inf_ctx.unify(ty, self.inf_ctx.bool_ty()) {
                        Ok(()) => (),
                        Err(err) => self
                            .inf_ctx
                            .diagnostics
                            .push_regular_diagnostic(err, cond.span.clone()),
                    },
                    Err(err) => self
                        .inf_ctx
                        .diagnostics
                        .push_regular_diagnostic(err, cond.span.clone()),
                }
                self.type_check_stmt(then);
                else_.as_ref().inspect(|else_| self.type_check_stmt(else_));
            }
            HirStmtKind::While { cond, body } => match self.inf_ctx.infer_expr(cond) {
                Ok(ty) => {
                    if let Err(err) = self.inf_ctx.unify(ty, self.inf_ctx.bool_ty()) {
                        self.inf_ctx
                            .diagnostics
                            .push_regular_diagnostic(err, cond.span.clone());
                        return;
                    }
                    self.type_check_stmt(body);
                }
                Err(err) => {
                    self.inf_ctx
                        .diagnostics
                        .push_regular_diagnostic(err, cond.span.clone());
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
    pub locals: BTreeMap<LocalId, Option<TypeRef>>,
}

fn type_check_hir<'db>(
    db: &'db dyn Db,
    hir: HirBody<'db>,
    packages: Arc<[Package<'db>]>,
) -> TypeCheckResults<'db> {
    TyCtx::new(
        db,
        hir.owner(db),
        hir.locals(db),
        hir.params(db),
        hir.zelf(db),
        packages,
    )
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
