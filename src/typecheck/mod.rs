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

use crate::common::symbols::Symbol;
use crate::hir::{HirMatchBranch, HirStructFieldPattern};
use crate::parse_tree::top_level::AstStructDefField;
use crate::parse_tree::type_expr::AstAnyTypeExpr;
use crate::typecheck::inference::implicit::AsAstImplCtx;
use crate::{
    Db,
    hir::{
        self, HirBody, HirExpr, HirId, HirPattern, HirPatternDesc, HirStmt,
        HirStmtKind, LocalId, LocalInfo, hir_body,
    },
    name_resolve::type_expr::{
        enum_item, get_templates_of_fun, templates_of_enum,
    },
    parse_tree::top_level::{AstEnumVariantKind, AstTemplateArg},
    ril::{self, FunctionId, InternedFunctionId, TypeDefId, TypeRef},
    typecheck::inference::{
        InferTy, InferenceCtx, UnificationError,
        constraints::InferenceConstraintKind, implicit::ImplicitContext,
        var::InferVar,
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
            variadic: infos.variadic,
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
            let txt = format!("<UNSOLVED> {}", unsolved.kind.display(self.db));
            dbg!("TODO: err here !", txt);
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
        self.inf_ctx
            .allocate_type_ref(&ret, &self.inf_ctx.implicit_ctx())
    }

    fn inner_type_of_pattern(
        &mut self,
        pattern: &HirPattern,
        loc_inners: &HashMap<LocalId, InferVar>,
        binds_like: InferVar,
    ) -> InferTy {
        if let Some(inferred) =
            self.inf_ctx.inferred_patterns.get(&PatternId(pattern.id))
        {
            return inferred.clone();
        }
        let mut compute = || {
            match &pattern.data {
                HirPatternDesc::Bind { id, .. } => {
                    // TODO: is this right ?
                    InferTy::Var(*loc_inners.get(id).expect(
                        "Internal error: local referenced in hir but not found",
                    ))
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
                        .expect(
                            "Variants of enum should already have been checked",
                        );
                    match (fields, &variant.kind) {
                        (
                            hir::HirPatternConstructorArgs::None,
                            AstEnumVariantKind::Unit,
                        ) => (),
                        (
                            hir::HirPatternConstructorArgs::StructFields(
                                hir_fields,
                            ),
                            AstEnumVariantKind::StructLike(ast_fields),
                        ) => {
                            assert_eq!(hir_fields.len(), ast_fields.len());
                            let mut fields: HashMap<
                                Symbol,
                                &AstStructDefField,
                            > = HashMap::new();
                            ast_fields.iter().for_each(|field| {
                                fields.insert(field.name, field);
                            });

                            for hir in hir_fields {
                                match hir {
                                    HirStructFieldPattern::Rebind {
                                        name,
                                        pattern,
                                    } => {
                                        if let Some(ast) = fields.get(name) {
                                            let ty = ctx
                                                .resolve(self.db, &ast.ty.data)
                                                .unwrap_or(TypeRef::Error);
                                            let infer_ty = self
                                                .inf_ctx
                                                .allocate_type_ref(&ty, &ctx);
                                            let pat_ty = self
                                                .inf_ctx
                                                .infer_pattern(pattern, None)
                                                .unwrap();
                                            let to_unify = self
                                                .inf_ctx
                                                .emit_binds_like_constraint(
                                                    binds_like, infer_ty,
                                                );
                                            if let Err(_err) =
                                                self.inf_ctx.unify(
                                                    InferTy::Var(to_unify),
                                                    pat_ty,
                                                )
                                            {
                                                todo!()
                                            }
                                        } else {
                                            todo!()
                                        }
                                    }
                                    HirStructFieldPattern::Name {
                                        id,
                                        name,
                                    } => {
                                        if let Some(ast) = fields.get(name) {
                                            let ty = ctx
                                                .resolve(self.db, &ast.ty.data)
                                                .unwrap_or(TypeRef::Error);
                                            let infer_ty = self
                                                .inf_ctx
                                                .allocate_type_ref(&ty, &ctx);
                                            let local_ty =
                                                self.inf_ctx.infer_local(*id);
                                            let to_unify = self
                                                .inf_ctx
                                                .emit_binds_like_constraint(
                                                    binds_like, infer_ty,
                                                );
                                            if let Err(_err) =
                                                self.inf_ctx.unify(
                                                    InferTy::Var(to_unify),
                                                    local_ty,
                                                )
                                            {
                                                todo!()
                                            }
                                        } else {
                                            todo!()
                                        }
                                    }
                                }
                            }
                        }
                        (
                            hir::HirPatternConstructorArgs::TupleFields(
                                hir_patterns,
                            ),
                            AstEnumVariantKind::TupleLike(ast_patterns),
                        ) => {
                            assert!(hir_patterns.len() == ast_patterns.len());
                            for (hir_pattern, ast_pattern) in
                                hir_patterns.iter().zip(ast_patterns.iter())
                            {
                                let pat_ty = self.inner_type_of_pattern(
                                    hir_pattern,
                                    loc_inners,
                                    binds_like,
                                );
                                let Some(ast_ty) =
                                    self.inf_ctx.allocate_ast_type_expr(
                                        &ast_pattern.data,
                                        &ctx,
                                    )
                                else {
                                    unreachable!()
                                };

                                if let Err(err) =
                                    self.inf_ctx.unify(pat_ty, ast_ty)
                                {
                                    dbg!("TODO: err here !", err);
                                }
                            }
                        }
                        _ => unreachable!(
                            "Mismatch between AST variant kind decl and case"
                        ),
                    }
                    t_ref
                }
                HirPatternDesc::IntLit(_) => {
                    InferTy::Var(self.inf_ctx.emit_intlike_constraint())
                }
                HirPatternDesc::Error => InferTy::Var(self.inf_ctx.fresh_var()),
            }
        };
        let res = compute();
        self.inf_ctx
            .inferred_patterns
            .insert(PatternId(pattern.id), res.clone());
        res
    }

    fn type_check_match(
        &mut self,
        scrutinee: &HirExpr,
        branches: &[HirMatchBranch],
    ) {
        let (typeof_scrut, err) = self.type_check_expr(scrutinee);
        if let Some(err) = err {
            dbg!("TODO: err here !", err);
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
        if let Err(err) = self
            .inf_ctx
            .unify(typeof_scrut.clone(), InferTy::Var(typeof_scrut_var))
        {
            dbg!("TODO: err here !", err);
        }

        for branch in branches {
            let mut loc_inners = HashMap::new();
            for local in &branch.locals {
                let typeof_local = self.inf_ctx.local_var(*local);
                let inner_var = self.inf_ctx.fresh_var();
                self.inf_ctx.emit_constraint(
                    InferenceConstraintKind::BindsLike {
                        ty: typeof_local,
                        inner: InferTy::Var(inner_var),
                        like: typeof_scrut_var,
                    },
                );
                loc_inners.insert(*local, inner_var);
            }

            let inner_type_of_pattern = self.inner_type_of_pattern(
                &branch.pattern,
                &loc_inners,
                typeof_scrut_var,
            );

            self.inf_ctx
                .emit_constraint(InferenceConstraintKind::IsInner {
                    inner: inner_type_of_pattern,
                    ref_ty: typeof_scrut.clone(),
                });

            self.type_check_stmt(&branch.body);
        }
    }

    fn type_check_let(
        &mut self,
        pattern: &HirPattern,
        ty_annotation: Option<&AstAnyTypeExpr>,
        init: &HirExpr,
    ) {
        let (init_ty, err) = self.type_check_expr(init);
        if let Some(err) = err {
            dbg!("TODO: err here !", err);
        }
        match self.inf_ctx.infer_pattern(pattern, None) {
            Ok(pattern_ty) => match init_ty {
                TyRef::Inf(infer_ty) => {
                    if let Err(err) =
                        self.inf_ctx.unify(infer_ty.clone(), pattern_ty)
                    {
                        dbg!("TODO: err here !", err);
                    };
                    if let Some(annotation) = ty_annotation
                        && let Some(annotation) = annotation.as_known()
                        && let Some(annotated) =
                            self.inf_ctx.allocate_ast_type_expr(
                                &annotation.data,
                                self.inf_ctx.implicit_ctx().as_ref(),
                            )
                        && let Err(err) =
                            self.inf_ctx.unify(infer_ty, annotated)
                    {
                        dbg!("TODO: err here !", err);
                    }
                }
                TyRef::Error => (),
            },
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
                    dbg!("TODO: err here !", err);
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
                            if let Err(err) = self
                                .inf_ctx
                                .unify(ret_ty.clone(), infer_ty.clone())
                            {
                                let fmt = format!(
                                    "Cannot return {} form a function expected to return {}",
                                    self.inf_ctx
                                        .find(&infer_ty)
                                        .to_string(self.db),
                                    self.inf_ctx
                                        .find(&ret_ty)
                                        .to_string(self.db)
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
                    Ok(ty) => {
                        match self.inf_ctx.unify(ty, self.inf_ctx.bool_ty()) {
                            Ok(()) => (),
                            Err(err) => {
                                dbg!("TODO: err here !", err);
                            }
                        }
                    }
                    Err(err) => {
                        dbg!("TODO: err here !", err);
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
                            dbg!("TODO: err here !", err);

                            return;
                        }
                        self.type_check_stmt(body);
                    }
                    Err(err) => {
                        dbg!("TODO: err here !", err);
                    }
                }
            }
            HirStmtKind::Block(stmts) => {
                stmts.iter().for_each(|stmt| self.type_check_stmt(stmt))
            }
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
    pub expr_types: BTreeMap<ExprId, TypeRef>,
    pub pat_types: BTreeMap<PatternId, TypeRef>,
    pub place_types: BTreeMap<PlaceId, TypeRef>,
    pub call_infos: BTreeMap<ExprId, CallInfos>,
    pub locals: BTreeMap<LocalId, Option<TypeRef>>,
}

fn type_check_hir<'db>(
    db: &'db dyn Db,
    hir: HirBody<'db>,
) -> TypeCheckResults<'db> {
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
