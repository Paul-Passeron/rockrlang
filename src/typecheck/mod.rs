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

use itertools::Itertools;
use salsa::Accumulator;

use crate::compiler::diagnostic::Diag;
use crate::hir::{HirMatchBranch, HirPatternDesc};
use crate::parse_tree::type_expr::AstAnyTypeExpr;
use crate::ril::TypeId;
use crate::{
    Db,
    hir::{
        HirBody, HirExpr, HirId, HirPattern, HirStmt, HirStmtKind, LocalId,
        LocalInfo, hir_body,
    },
    name_resolve::type_expr::get_templates_of_fun,
    parse_tree::top_level::AstTemplateArg,
    ril::{FunctionId, InternedFunctionId, TypeRef},
    typecheck::inference::{InferTy, InferenceCtx, UnificationError},
};

pub mod conformance;
pub mod inference;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReceiverAdjustment {
    /// The receiver is the expr as is
    None,

    /// The receiver is a ref to the expression
    Ref,

    /// The receiver is a mutable ref to the expression
    MutRef,

    /// The receiver is the expr dereferenced \.0 times
    Deref(usize),

    /// The receiver is a ref to the expr dereferenced \.0 times
    DerefThenRef(usize),

    /// The receiver is a mutable ref to the expr dereferenced \.0 times
    DerefThenMutRef(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallKind {
    Direct,                                    // Regular function call
    Method { adjustment: ReceiverAdjustment }, // foo.bar(...)
    Static,                                    // Foo::bar(...) with no self
}

#[derive(Clone)]
pub(super) struct InferCallInfos {
    expr_id: ExprId,
    callee: FunctionId,
    substitution: Box<[InferTy]>,
    call_kind: CallKind,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CallInfos {
    pub expr_id: ExprId,
    pub callee: FunctionId,
    pub substitution: Vec<TypeRef>,
    pub call_kind: CallKind,
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
        let local_ids =
            locals.iter().map(|local| local.id).collect::<Box<[_]>>();
        let inf_ctx = InferenceCtx::new(db, &local_ids, function, zelf, params);

        Self { db, function, locals, params, templates, inf_ctx }
    }

    fn concretize_infos(&mut self, infos: InferCallInfos) -> CallInfos {
        CallInfos {
            expr_id: infos.expr_id,
            callee: infos.callee,
            substitution: infos
                .substitution
                .into_iter()
                .map(|ty| self.inf_ctx.solve(ty).unwrap_or(TypeRef::Error))
                .collect_vec()
                .into_iter()
                .map(|ty| self.canon_type(ty))
                .collect(),
            call_kind: infos.call_kind,
        }
    }

    fn canon_type(&self, ty: TypeRef) -> TypeRef {
        let db = self.db;
        match ty {
            TypeRef::Concrete(type_id) => TypeRef::Concrete(TypeId::new(
                db,
                type_id.def(db),
                type_id
                    .args(db)
                    .iter()
                    .map(|ty| self.canon_type(*ty))
                    .collect(),
            )),
            TypeRef::Zelf => {
                self.function.parent(db).get_canonical_zelf(db).unwrap()
            }
            _ => ty,
        }
    }

    fn finalize(mut self) -> TypeCheckResults<'db> {
        if let Err((cstr, err)) = self.inf_ctx.solve_constraints() {
            Diag::generic_error(
                format!(
                    "unification error while solving `{}`: {}",
                    cstr.kind.display(&self.inf_ctx),
                    err.display(self.db)
                ),
                self.function.span(self.db),
            )
            .accumulate(self.db);
        }

        // Temporary
        let unsolveds = self.inf_ctx.unsolved_constraints();
        if !unsolveds.is_empty() {
            eprintln!(
                "{}: Unsolved constraints",
                self.function.span(self.db).start().loc_info(self.db)
            );
            for unsolved in unsolveds {
                eprintln!(
                    "<UNSOLVED> {}",
                    unsolved.kind.display(&self.inf_ctx)
                );
            }
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

    fn type_check_expr(
        &mut self,
        expr: &HirExpr,
    ) -> (TyRef, Option<UnificationError>) {
        let (ty, err) = match self.inf_ctx.infer_expr(expr) {
            Ok(infer_ty) => (TyRef::Inf(infer_ty), None),
            Err(err) => (TyRef::Error, Some(err)),
        };
        (ty, err)
    }

    fn get_ret_ty(&mut self) -> InferTy {
        let ret = self.function.ret_ty(self.db);
        self.inf_ctx.allocate_type_ref(ret, &self.inf_ctx.implicit_ctx())
    }

    fn type_check_match(
        &mut self,
        scrutinee: &HirExpr,
        branches: &[HirMatchBranch],
    ) {
        let (typeof_scrut, err) = self.type_check_expr(scrutinee);
        if let Some(err) = err {
            Diag::generic_error(
                format!("unification error: `{}`", err.display(self.db)),
                scrutinee.span,
            )
            .accumulate(self.db);
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
            self.inf_ctx.emit_is_inner_constraint(pat_ty, typeof_scrut.clone());

            if let Some(guard) = &branch.guard {
                let inferred = self.inf_ctx.infer_expr(guard);
                let guard_ty = match inferred {
                    Ok(ty) => ty,
                    Err(err) => {
                        error.1 = Some(err);
                        InferTy::Var(self.inf_ctx.fresh_var())
                    }
                };
                if let Err(err) =
                    self.inf_ctx.unify(guard_ty, self.inf_ctx.bool_ty())
                {
                    // This is safe to do because this can fail only if the one
                    // above didn't.
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
            Err(err) => {
                Diag::generic_error(
                    format!("unification error: `{}`", err.display(self.db)),
                    init.span,
                )
                .accumulate(self.db);
                InferTy::Var(self.inf_ctx.fresh_var())
            }
        };
        match self.inf_ctx.infer_pattern(pattern, Some(init_ty.clone())) {
            Ok(pattern_ty) => {
                match &pattern.data {
                    HirPatternDesc::Tuple(_)
                    | HirPatternDesc::DestructureBinding { .. } => {
                        self.inf_ctx.emit_is_inner_constraint(
                            pattern_ty,
                            init_ty.clone(),
                        );
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
                    && let Some(annotated) =
                        self.inf_ctx.allocate_ast_type_expr(
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
                Diag::generic_error(
                    format!("unification error: `{}`", err.display(self.db)),
                    pattern.span,
                )
                .accumulate(self.db);
            }
        }
    }

    fn type_check_stmt(&mut self, stmt: &HirStmt) {
        match &stmt.kind {
            HirStmtKind::Let { pattern, ty_annotation, init, .. } => {
                self.type_check_let(pattern, ty_annotation.as_ref(), init);
            }
            HirStmtKind::Match { scrutinee, branches } => {
                self.type_check_match(scrutinee, branches);
            }
            HirStmtKind::Assign { lhs, rhs } => {
                let (rhs_ty, rhs_err) = self.type_check_expr(rhs);
                if let Some(rhs_err) = rhs_err {
                    Diag::generic_error(
                        format!(
                            "unification error: `{}`",
                            rhs_err.display(self.db)
                        ),
                        rhs.span,
                    )
                    .accumulate(self.db);
                }
                match self.inf_ctx.infer_place(lhs) {
                    Ok(lhs_ty) => match rhs_ty {
                        TyRef::Inf(rhs_ty) => {
                            if let Err(err) = self
                                .inf_ctx
                                .unify(lhs_ty.clone(), rhs_ty.clone())
                            {
                                let lstr = self
                                    .inf_ctx
                                    .find(&lhs_ty)
                                    .to_string(self.db);
                                let rstr = self
                                    .inf_ctx
                                    .find(&rhs_ty)
                                    .to_string(self.db);
                                Diag::generic_error(
                                    format!(
                                        "Cannot assign value of type {rstr} to a place of type {lstr}: {}",
                                        err.display(self.db)
                                    ),
                                    stmt.span,
                                )
                                .accumulate(self.db);
                            }
                        }
                        TyRef::Error => (),
                    },
                    Err(err) => {
                        Diag::generic_error(
                            format!(
                                "unification error: `{}`",
                                err.display(self.db)
                            ),
                            lhs.span,
                        )
                        .accumulate(self.db);
                    }
                }
            }
            HirStmtKind::Expr(hir_expr) => {
                let (_, err) = self.type_check_expr(hir_expr);
                if let Some(err) = err {
                    Diag::generic_error(
                        format!(
                            "unification error: `{}`",
                            err.display(self.db)
                        ),
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
                        Diag::generic_error(
                            format!(
                                "unification error: `{}`",
                                err.display(self.db)
                            ),
                            expr.span,
                        )
                        .accumulate(self.db);
                    };

                    match ty {
                        TyRef::Inf(infer_ty) => {
                            if let Err(err) = self
                                .inf_ctx
                                .unify(ret_ty.clone(), infer_ty.clone())
                            {
                                let _ = err;
                                Diag::generic_error(
                                    format!(
                                        "Cannot return {} form a function expected to return {}",
                                        self.inf_ctx
                                            .find(&infer_ty)
                                            .to_string(self.db),
                                        self.inf_ctx
                                            .find(&ret_ty)
                                            .to_string(self.db)
                                    ),
                                    expr.span,
                                )
                                .accumulate(self.db);
                            }
                        }
                        TyRef::Error => {
                            Diag::generic_error(
                                format!(
                                    "Cannot return error type form a function expected to return {}",
                                    self.inf_ctx.find(&ret_ty).to_string(self.db)
                                ),
                                expr.span,
                            )
                            .accumulate(self.db);
                        }
                    }
                } else {
                    let void_ty = self.inf_ctx.void_ty();
                    if ret_ty != void_ty {
                        Diag::generic_error(
                            format!(
                                "Cannot have an empty return from a function expected to return {}",
                                self.inf_ctx.find(&ret_ty).to_string(self.db)
                            ),
                            stmt.span,
                        )
                        .accumulate(self.db);
                    }
                }
            }
            HirStmtKind::If { cond, then, else_ } => {
                match self.inf_ctx.infer_expr(cond) {
                    Ok(ty) => {
                        if let Err(err) =
                            self.inf_ctx.unify(ty, self.inf_ctx.bool_ty())
                        {
                            Diag::generic_error(
                                format!(
                                    "if condition must be of type bool: {}",
                                    err.display(self.db)
                                ),
                                cond.span,
                            )
                            .accumulate(self.db);
                        }
                    }
                    Err(err) => {
                        Diag::generic_error(
                            format!(
                                "unification error: `{}`",
                                err.display(self.db)
                            ),
                            cond.span,
                        )
                        .accumulate(self.db);
                    }
                }
                self.type_check_stmt(then);
                else_.as_ref().inspect(|else_| self.type_check_stmt(else_));
            }
            HirStmtKind::While { cond, body } => {
                match self.inf_ctx.infer_expr(cond) {
                    Ok(ty) => {
                        if let Err(err) =
                            self.inf_ctx.unify(ty, self.inf_ctx.bool_ty())
                        {
                            Diag::generic_error(
                                format!(
                                    "while condition must be of type bool: {}",
                                    err.display(self.db)
                                ),
                                cond.span,
                            )
                            .accumulate(self.db);

                            return;
                        }
                        self.type_check_stmt(body);
                    }
                    Err(err) => {
                        Diag::generic_error(
                            format!(
                                "unification error: `{}`",
                                err.display(self.db)
                            ),
                            cond.span,
                        )
                        .accumulate(self.db);
                    }
                }
            }
            HirStmtKind::Block(stmts) => {
                stmts.iter().for_each(|stmt| self.type_check_stmt(stmt))
            }
            HirStmtKind::Defer(stmt) => self.type_check_stmt(stmt),
            HirStmtKind::Break => (),
        }
        if let Err((cstr, err)) = self.inf_ctx.solve_constraints() {
            Diag::generic_error(
                format!(
                    "unification error while solving `{}`: {}",
                    cstr.kind.display(&self.inf_ctx),
                    err.display(self.db)
                ),
                stmt.span,
            )
            .accumulate(self.db);
        }
    }

    fn type_check(mut self, stmts: &'db [HirStmt]) -> TypeCheckResults<'db> {
        stmts.iter().for_each(|stmt| self.type_check_stmt(stmt));
        self.finalize()
    }
}

#[salsa::tracked]
pub struct TypeCheckResults<'db> {
    #[returns(ref)]
    pub expr_types: BTreeMap<ExprId, TypeRef>,
    #[returns(ref)]
    pub pat_types: BTreeMap<PatternId, TypeRef>,
    #[returns(ref)]
    pub place_types: BTreeMap<PlaceId, TypeRef>,
    #[returns(ref)]
    pub call_infos: BTreeMap<ExprId, CallInfos>,
    #[returns(ref)]
    pub locals: BTreeMap<LocalId, Option<TypeRef>>,
}

fn type_check_hir<'db>(
    db: &'db dyn Db,
    hir: HirBody<'db>,
) -> TypeCheckResults<'db> {
    TyCtx::new(db, hir.owner(db), hir.locals(db), hir.params(db), hir.zelf(db))
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
