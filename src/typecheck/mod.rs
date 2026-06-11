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

use std::{collections::BTreeMap, panic, sync::Arc};

use salsa::Accumulator;

use crate::compiler::diagnostic::Diag;
use crate::hir::{HirMatchBranch, HirPatternDesc};
use crate::parse_tree::type_expr::AstAnyTypeExpr;
use crate::{
    Db,
    hir::{
        HirBody, HirExpr, HirId, HirPattern, HirStmt, HirStmtKind, LocalId, LocalInfo,
        hir_body,
    },
    name_resolve::type_expr::get_templates_of_fun,
    parse_tree::top_level::AstTemplateArg,
    ril::{FunctionId, InternedFunctionId, TypeRef},
    typecheck::inference::{InferTy, InferenceCtx, UnificationError},
};

pub mod inference;

#[derive(Clone)]
pub(super) struct InferCallInfos {
    expr_id: ExprId,
    callee: FunctionId,
    substitution: Box<[InferTy]>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CallInfos {
    pub expr_id: ExprId,
    pub callee: FunctionId,
    pub substitution: Vec<TypeRef>,
}

#[derive(Clone)]
#[allow(dead_code)]
struct TyCtx<'db> {
    db: &'db dyn Db,
    function: FunctionId,
    locals: &'db [LocalInfo],
    params: &'db [LocalId],

    templates: Arc<[AstTemplateArg]>,

    inf_ctx: InferenceCtx<'db>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExprId(pub HirId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatternId(pub HirId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlaceId(pub HirId);

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
    ) -> Self {
        let templates = get_templates_of_fun(db, function.interned());
        let local_ids = locals.iter().map(|local| local.id).collect::<Box<[_]>>();
        let inf_ctx = InferenceCtx::new(db, &local_ids, function, zelf, params);

        Self {
            db,
            function,
            locals,
            params,
            templates,
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
        }
    }

    fn finalize(mut self) -> TypeCheckResults<'db> {
        if let Err((cstr, err)) = self.inf_ctx.solve_constraints() {
            let err = err.display(self.db).to_string();
            let kind = cstr.kind.display(self.db).to_string();
            dbg!(err, kind);
        }

        // Temporary
        let unsolveds = self.inf_ctx.unsolved_constraints();
        for unsolved in unsolveds {
            eprintln!("<UNSOLVED> {}", unsolved.kind.display(self.db));
        }

        let drain = std::mem::take(&mut self.inf_ctx.inferred_exprs)
            .into_iter()
            .collect::<Box<[_]>>();
        let node_types = drain
            .into_iter()
            .map(|(id, infer_ty)| {
                (id, self.inf_ctx.solve(infer_ty).unwrap_or(TypeRef::Unknown))
            })
            .collect::<BTreeMap<_, _>>();

        let drain = std::mem::take(&mut self.inf_ctx.inferred_patterns)
            .into_iter()
            .collect::<Box<[_]>>();
        let pat_types = drain
            .into_iter()
            .map(|(id, infer_ty)| {
                (id, self.inf_ctx.solve(infer_ty).unwrap_or(TypeRef::Unknown))
            })
            .collect::<BTreeMap<_, _>>();

        let drain = std::mem::take(&mut self.inf_ctx.inferred_places)
            .into_iter()
            .collect::<Box<[_]>>();
        let place_types = drain
            .into_iter()
            .map(|(id, infer_ty)| {
                (id, self.inf_ctx.solve(infer_ty).unwrap_or(TypeRef::Unknown))
            })
            .collect::<BTreeMap<_, _>>();

        let call_infos = self
            .inf_ctx
            .drain_call_infos()
            .into_iter()
            .map(|(id, infos)| (id, self.concretize_infos(infos)))
            .collect();

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

        TypeCheckResults::new(
            self.db,
            node_types,
            pat_types,
            place_types,
            call_infos,
            locals,
        )
    }

    fn type_check_expr(&mut self, expr: &HirExpr) -> (TyRef, Option<UnificationError>) {
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

    fn type_check_match(&mut self, scrutinee: &HirExpr, branches: &[HirMatchBranch]) {
        let (typeof_scrut, err) = self.type_check_expr(scrutinee);
        if let Some(err) = err {
            dbg!("TODO: err here !", err);
        }
        let typeof_scrut = match typeof_scrut {
            TyRef::Inf(infer_ty) => infer_ty,
            TyRef::Error => self.inf_ctx.fresh_var().into(),
        };

        let mut errors = vec![];

        for branch in branches {
            let mut error = (None, None);
            let inferred = self
                .inf_ctx
                .infer_pattern(&branch.pattern, Some(typeof_scrut.clone()));
            let pat_ty = match inferred {
                Ok(ty) => ty,
                Err(err) => {
                    error.0 = Some(err);
                    InferTy::Var(self.inf_ctx.fresh_var())
                }
            };
            self.inf_ctx
                .emit_is_inner_constraint(pat_ty, typeof_scrut.clone());

            if let Some(guard) = &branch.guard {
                let inferred = self.inf_ctx.infer_expr(guard);
                let guard_ty = match inferred {
                    Ok(ty) => ty,
                    Err(err) => {
                        error.1 = Some(err);
                        InferTy::Var(self.inf_ctx.fresh_var())
                    }
                };
                if let Err(err) = self.inf_ctx.unify(guard_ty, self.inf_ctx.bool_ty()) {
                    // This is safe to do because this can fail only if the one above
                    // didn't.
                    error.1 = Some(err);
                }
            }

            errors.push(error);

            self.type_check_stmt(&branch.body);
        }

        for (err, _branch) in errors.into_iter().zip(branches) {
            match err {
                (None, None) => (),
                _ => todo!(),
            }
        }
    }

    fn type_check_let(
        &mut self,
        pattern: &HirPattern,
        ty_annotation: Option<&AstAnyTypeExpr>,
        init: &HirExpr,
    ) {
        let init_ty = match self.inf_ctx.infer_expr(init) {
            Ok(ty) => ty,
            Err(err) => todo!("{}", err.display(self.db)),
        };
        match self.inf_ctx.infer_pattern(pattern, Some(init_ty.clone())) {
            Ok(pattern_ty) => {
                match &pattern.data {
                    HirPatternDesc::Tuple(_)
                    | HirPatternDesc::DestructureBinding { .. } => {
                        self.inf_ctx
                            .emit_is_inner_constraint(pattern_ty, init_ty.clone());
                    }
                    HirPatternDesc::Constructor { .. } => {
                        todo!("Not allowed here")
                    }
                    _ => {
                        self.inf_ctx
                            .unify(init_ty.clone(), pattern_ty)
                            .expect("TODO");
                    }
                }
                if let Some(annotation) = ty_annotation
                    && let Some(annotation) = annotation.as_known()
                    && let Some(annotated) = self.inf_ctx.allocate_ast_type_expr(
                        &annotation.data,
                        self.inf_ctx.implicit_ctx().as_ref(),
                    )
                    && let Err(_err) =
                        self.inf_ctx.unify(init_ty.clone(), annotated.clone())
                {
                    Diag::generic_error(
                        format!(
                            "Annotation does not match the type: {} vs {}",
                            self.inf_ctx.find(&annotated).to_string(self.db),
                            self.inf_ctx.find(&init_ty).to_string(self.db),
                        ),
                        pattern.span.start().span(init.span.end()),
                    )
                    .accumulate(self.db);
                }
            }
            Err(err) => {
                dbg!("TODO: err here !", err);
            }
        }
    }

    fn type_check_stmt(&mut self, stmt: &HirStmt) {
        match &stmt.kind {
            HirStmtKind::Let {
                pattern,
                ty_annotation,
                init,
                ..
            } => {
                self.type_check_let(pattern, ty_annotation.as_ref(), init);
            }
            HirStmtKind::Match {
                scrutinee,
                branches,
            } => {
                self.type_check_match(scrutinee, branches);
            }
            HirStmtKind::Assign { lhs, rhs } => {
                let (rhs_ty, rhs_err) = self.type_check_expr(rhs);
                if let Some(rhs_err) = rhs_err {
                    dbg!("TODO: err here !", rhs_err);
                }
                match self.inf_ctx.infer_place(lhs) {
                    Ok(lhs_ty) => match rhs_ty {
                        TyRef::Inf(rhs_ty) => {
                            if let Err(err) =
                                self.inf_ctx.unify(lhs_ty.clone(), rhs_ty.clone())
                            {
                                let lstr = self.inf_ctx.find(&lhs_ty).to_string(self.db);
                                let rstr = self.inf_ctx.find(&rhs_ty).to_string(self.db);
                                dbg!("TODO: err here !", err, lstr, rstr);
                            }
                        }
                        TyRef::Error => (),
                    },
                    Err(err) => {
                        dbg!("TODO: err here !", err);
                    }
                }
            }
            HirStmtKind::Expr(hir_expr) => {
                let (_, err) = self.type_check_expr(hir_expr);
                if let Some(err) = err {
                    Diag::generic_error(
                        format!("unification error: `{}`", err.display(self.db)),
                        hir_expr.span,
                    )
                    .accumulate(self.db);
                }
            }
            HirStmtKind::Return(hir_expr) => {
                let ret_ty = self.get_ret_ty();
                if let Some(expr) = &hir_expr {
                    let (ty, err) = self.type_check_expr(expr);
                    if let Some(err) = err {
                        dbg!("TODO: err here !", err);
                    };

                    match ty {
                        TyRef::Inf(infer_ty) => {
                            if let Err(err) =
                                self.inf_ctx.unify(ret_ty.clone(), infer_ty.clone())
                            {
                                let fmt = format!(
                                    "Cannot return {} form a function expected to return {}",
                                    self.inf_ctx.find(&infer_ty).to_string(self.db),
                                    self.inf_ctx.find(&ret_ty).to_string(self.db)
                                );
                                dbg!("TODO: err here !", err, fmt);
                            }
                        }
                        TyRef::Error => {
                            let fmt = format!(
                                "Cannot return error type form a function expected to return {}",
                                ret_ty.to_string(self.db)
                            );
                            dbg!("TODO: err here !", fmt);
                        }
                    }
                } else {
                    let void_ty = self.inf_ctx.void_ty();
                    if ret_ty != void_ty {
                        let fmt = format!(
                            "Cannot have an empty return from a function expected to return {}",
                            ret_ty.to_string(self.db)
                        );
                        dbg!("TODO: err here !", fmt);
                    }
                }
            }
            HirStmtKind::If { cond, then, else_ } => {
                match self.inf_ctx.infer_expr(cond) {
                    Ok(ty) => match self.inf_ctx.unify(ty, self.inf_ctx.bool_ty()) {
                        Ok(()) => (),
                        Err(err) => {
                            dbg!("TODO: err here !", err);
                        }
                    },
                    Err(err) => {
                        dbg!("TODO: err here !", err);
                    }
                }
                self.type_check_stmt(then);
                else_.as_ref().inspect(|else_| self.type_check_stmt(else_));
            }
            HirStmtKind::While { cond, body } => match self.inf_ctx.infer_expr(cond) {
                Ok(ty) => {
                    if let Err(err) = self.inf_ctx.unify(ty, self.inf_ctx.bool_ty()) {
                        dbg!("TODO: err here !", err);

                        return;
                    }
                    self.type_check_stmt(body);
                }
                Err(err) => {
                    dbg!("TODO: err here !", err);
                }
            },
            HirStmtKind::Block(stmts) => {
                stmts.iter().for_each(|stmt| self.type_check_stmt(stmt))
            }
            HirStmtKind::Defer(stmt) => self.type_check_stmt(stmt),
            HirStmtKind::Break => (),
        }
        if let Err(err) = self.inf_ctx.solve_constraints() {
            dbg!(err);
        }
    }

    fn type_check(mut self, stmts: &'db [HirStmt]) -> TypeCheckResults<'db> {
        stmts.iter().for_each(|stmt| self.type_check_stmt(stmt));
        self.finalize()
    }
}

#[salsa::tracked]
pub struct TypeCheckResults<'db> {
    pub expr_types: BTreeMap<ExprId, TypeRef>,
    pub pat_types: BTreeMap<PatternId, TypeRef>,
    pub place_types: BTreeMap<PlaceId, TypeRef>,
    pub call_infos: BTreeMap<ExprId, CallInfos>,
    pub locals: BTreeMap<LocalId, Option<TypeRef>>,
}

fn type_check_hir<'db>(db: &'db dyn Db, hir: HirBody<'db>) -> TypeCheckResults<'db> {
    TyCtx::new(
        db,
        hir.owner(db),
        hir.locals(db),
        hir.params(db),
        hir.zelf(db),
    )
    .type_check(hir.stmts(db))
}

#[salsa::tracked]
pub fn _type_check_function<'db>(
    db: &'db dyn Db,
    function: InternedFunctionId<'db>,
) -> Option<TypeCheckResults<'db>> {
    hir_body(db, function.into()).map(|hir| type_check_hir(db, hir))
}

pub fn type_check_function<'db>(
    db: &'db dyn Db,
    function: FunctionId,
) -> Option<TypeCheckResults<'db>> {
    _type_check_function(db, function.interned())
}
