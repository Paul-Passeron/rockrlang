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

use std::{collections::HashMap, sync::Arc};

use itertools::{Either, Itertools};
use la_arena::{Arena, Idx};

use crate::{
    Db,
    common::{location::Span, symbols::Symbol},
    hir::{
        self, HirBody, HirConstructorArgs, HirExpr, HirExprDesc,
        HirMatchBranch, HirPattern, HirPatternConstructorArgs, HirPatternDesc,
        HirPlace, HirPlaceKind, HirStmt, HirStmtKind, HirStructFieldPattern,
        LocalInfo, Mutability, PartialTypeRef,
    },
    name_resolve::type_expr::{enum_item, struct_item},
    ril::{
        BuiltinTypeId, FunctionId, ScopeOwnerId, TypeDefId, TypeId, TypeRef,
    },
    thir::{
        EnumRef, ExprId, ExprKind, FunctionRef, LocalId, PlaceBase, PlaceId,
        Projection, ScopeId, StructRef, Thir, ThirConstructorArgs, ThirExpr,
        ThirExprWithSetup, ThirLocal, ThirMatchBranch, ThirPattern, ThirPlace,
        ThirScope, stmt::ThirStmt,
    },
    typecheck::{
        self, PatternId, TypeCheckResults,
        inference::implicit::AstImplicitContext,
    },
};

use super::{Dispatch, ScopeKind, ThirPatternKind};

pub struct ThirBuilder<'db> {
    db: &'db dyn Db,
    locals: Arena<ThirLocal>,
    exprs: Arena<ThirExpr>,
    places: Arena<ThirPlace>,
    scopes: Arena<ThirScope>,
    local_map: HashMap<hir::LocalId, LocalId>,
}

#[allow(dead_code)]
impl<'db> ThirBuilder<'db> {
    fn new(db: &'db dyn Db) -> Self {
        Self {
            db,
            locals: Arena::new(),
            exprs: Arena::new(),
            places: Arena::new(),
            scopes: Arena::new(),
            local_map: HashMap::new(),
        }
    }

    pub fn new_local(&mut self, local: ThirLocal) -> LocalId {
        self.locals.alloc(local)
    }
    pub fn new_expr(&mut self, expr: ThirExpr) -> ExprId {
        self.exprs.alloc(expr)
    }
    pub fn new_place(&mut self, place: ThirPlace) -> PlaceId {
        self.places.alloc(place)
    }
    pub fn new_scope(&mut self, scope: ThirScope) -> ScopeId {
        self.scopes.alloc(scope)
    }

    pub fn get_local(&self, idx: Idx<ThirLocal>) -> &ThirLocal {
        &self.locals[idx]
    }
    pub fn get_expr(&self, idx: Idx<ThirExpr>) -> &ThirExpr {
        &self.exprs[idx]
    }
    pub fn get_place(&self, idx: Idx<ThirPlace>) -> &ThirPlace {
        &self.places[idx]
    }
    pub fn get_scope(&self, idx: Idx<ThirScope>) -> &ThirScope {
        &self.scopes[idx]
    }

    pub fn finalize(
        self,
        id: FunctionId,
        params: Vec<LocalId>,
        zelf: Option<LocalId>,
        stmts: Vec<ThirStmt>,
    ) -> Thir {
        Thir {
            id,
            places: self.places,
            exprs: self.exprs,
            locals: self.locals,
            scopes: self.scopes,
            params,
            zelf,
            root: stmts,
        }
    }

    fn register_hir_local(
        &mut self,
        infos: &LocalInfo,
        tc_results: TypeCheckResults<'_>,
    ) {
        let local = ThirLocal {
            ty: tc_results.locals(self.db)[&infos.id]
                .unwrap_or(crate::ril::TypeRef::Unknown),
            mutability: infos.mutability,
            span: infos.span,
            source: Some((infos.id, infos.name)),
        };
        let res = self.new_local(local);
        self.local_map.insert(infos.id, res);
    }

    fn with_synthetic_projection(
        &mut self,
        place: PlaceId,
        proj: Projection,
        ty: TypeRef,
    ) -> PlaceId {
        let mut place = self.get_place(place).clone();
        place.projections.push(proj);
        place.ty = ty;
        self.new_place(place)
    }

    fn with_projection(
        &mut self,
        place: PlaceId,
        proj: Projection,
        ty: TypeRef,
        span: Span,
    ) -> PlaceId {
        let mut place = self.get_place(place).clone();
        place.projections.push(proj);
        place.ty = ty;
        place.span = span;
        self.new_place(place)
    }

    fn new_synthetic_local(
        &mut self,
        ty: TypeRef,
        mutability: Mutability,
        span: Span,
    ) -> LocalId {
        self.new_local(ThirLocal {
            ty,
            mutability,
            span,
            source: None, // Synthetic local
        })
    }
}

pub fn thir_body_from_hir<'db>(
    db: &'db dyn Db,
    hir: HirBody<'db>,
    tc: TypeCheckResults<'db>,
) -> Thir {
    ThirTranslator::new(db, hir, tc).translate()
}

pub struct ThirTranslator<'db> {
    db: &'db dyn Db,
    hir: HirBody<'db>,
    tc: TypeCheckResults<'db>,
    scope_stack: Vec<ScopeId>,
}

impl<'db> ThirTranslator<'db> {
    pub fn new(
        db: &'db dyn Db,
        hir: HirBody<'db>,
        tc: TypeCheckResults<'db>,
    ) -> Self {
        Self {
            db,
            hir,
            tc,
            scope_stack: Vec::new(),
        }
    }

    pub fn translate(mut self) -> Thir {
        let mut b = ThirBuilder::new(self.db);
        self.hir
            .locals(self.db)
            .iter()
            .for_each(|infos| b.register_hir_local(infos, self.tc));
        let params = self
            .hir
            .params(self.db)
            .iter()
            .map(|param| b.local_map[param])
            .collect_vec();
        let zelf = self.hir.zelf(self.db).map(|param| b.local_map[&param]);
        let stmts = self
            .hir
            .stmts(self.db)
            .iter()
            .flat_map(|stmt| self.handle_stmt(&mut b, stmt))
            .collect_vec();
        b.finalize(self.hir.owner(self.db), params, zelf, stmts)
    }

    fn handle_break(&self, b: &ThirBuilder, span: Span) -> ThirStmt {
        match self.innermost_loop_scope(b) {
            Some(scope_id) => ThirStmt::brk(scope_id, span),
            None => {
                if self.scope_stack.is_empty() {
                    println!("Cannot break at function top-level")
                } else {
                    println!("Cannot break out of non-loop block")
                }
                ThirStmt::error(span)
            }
        }
    }

    fn handle_block(
        &mut self,
        b: &mut ThirBuilder,
        stmts: &Vec<HirStmt>,
        span: Span,
    ) -> ThirStmt {
        let (scope, stmts) =
            self.scoped(b, span, ScopeKind::Block, |this, b| {
                stmts
                    .iter()
                    .flat_map(|stmt| this.handle_stmt(b, stmt))
                    .collect_vec()
            });
        ThirStmt::block(scope, stmts, span)
    }

    fn handle_stmt(
        &mut self,
        b: &mut ThirBuilder,
        stmt: &HirStmt,
    ) -> Vec<ThirStmt> {
        match &stmt.kind {
            HirStmtKind::Let { pattern, init, .. } => {
                let mut res = vec![];
                let value = self.expr(b, init, &mut res);
                res.extend(
                    self.destructure_pattern_init(b, pattern, value, stmt.span),
                );
                res
            }
            HirStmtKind::Match {
                scrutinee,
                branches,
            } => {
                vec![self.handle_match(b, scrutinee, branches, stmt.span)]
            }
            HirStmtKind::Assign { lhs, rhs } => {
                let mut res = vec![];
                let place = self.place(b, lhs, &mut res);
                let value = self.expr(b, rhs, &mut res);
                res.push(ThirStmt::assign(place, value, stmt.span));
                res
            }
            HirStmtKind::Expr(hir_expr) => {
                let mut res = vec![];
                let expr = self.expr(b, hir_expr, &mut res);
                res.push(ThirStmt::expr(expr, hir_expr.span));
                res
            }
            HirStmtKind::Return(hir_expr) => {
                let mut res = vec![];
                let expr =
                    hir_expr.as_ref().map(|expr| self.expr(b, expr, &mut res));
                res.push(ThirStmt::ret(expr, stmt.span));
                res
            }
            HirStmtKind::If { cond, then, else_ } => {
                vec![self.handle_if_block(
                    b,
                    cond,
                    then,
                    else_.as_ref().map(Box::as_ref),
                    stmt.span,
                )]
            }
            HirStmtKind::While { cond, body } => {
                vec![self.handle_while(b, cond, body, stmt.span)]
            }
            HirStmtKind::Block(hir_stmts) => {
                vec![self.handle_block(b, hir_stmts, stmt.span)]
            }
            HirStmtKind::Defer(_) => {
                todo!("error diagnostic for unimplemented defer stmts")
            }
            HirStmtKind::Break => vec![self.handle_break(b, stmt.span)],
        }
    }

    fn push_scope(
        &mut self,
        b: &mut ThirBuilder,
        kind: ScopeKind,
        span: Span,
    ) -> ScopeId {
        let scope_id = b.new_scope(ThirScope { kind, span });
        self.scope_stack.push(scope_id);
        scope_id
    }

    fn pop_scope(&mut self) -> Option<ScopeId> {
        self.scope_stack.pop()
    }

    fn handle_if_block(
        &mut self,
        b: &mut ThirBuilder<'_>,
        cond: &HirExpr,
        then: &HirStmt,
        else_: Option<&HirStmt>,
        span: Span,
    ) -> ThirStmt {
        let thir_cond = self.expr_with_setup(b, cond);

        let (then_scope, then_stmts) =
            self.scoped(b, then.span, ScopeKind::Block, |this, b| {
                this.handle_stmt(b, then)
            });

        let (else_stmts, else_scope) = match else_ {
            Some(stmt) => {
                let (else_scope, else_stmts) =
                    self.scoped(b, then.span, ScopeKind::Block, |this, b| {
                        this.handle_stmt(b, stmt)
                    });
                (Some(else_stmts), Some(else_scope))
            }
            None => (None, None),
        };

        ThirStmt::ifte(
            thir_cond, then_stmts, then_scope, else_stmts, else_scope, span,
        )
    }

    fn handle_while(
        &mut self,
        b: &mut ThirBuilder<'_>,
        cond: &HirExpr,
        body: &HirStmt,
        span: Span,
    ) -> ThirStmt {
        let cond = self.expr_with_setup(b, cond);
        let (scope, body) = self.scoped(b, span, ScopeKind::Loop, |this, b| {
            this.handle_stmt(b, body)
        });
        ThirStmt::whl(cond, scope, body, span)
    }

    fn handle_branch(
        &mut self,
        b: &mut ThirBuilder,
        branch: &HirMatchBranch,
    ) -> ThirMatchBranch {
        let pattern = self.pat(b, &branch.pattern);
        let guard = branch
            .guard
            .as_ref()
            .map(|guard| self.expr_with_setup(b, guard));
        let (body_scope, body) =
            self.scoped(b, branch.body.span, ScopeKind::Block, |this, b| {
                this.handle_stmt(b, &branch.body)
            });
        ThirMatchBranch {
            pattern,
            guard,
            body_scope,
            body,
        }
    }

    fn handle_match(
        &mut self,
        b: &mut ThirBuilder,
        scrutinee: &HirExpr,
        branches: &[HirMatchBranch],
        span: Span,
    ) -> ThirStmt {
        let scrut = self.expr_with_setup(b, scrutinee);
        let branches = branches
            .iter()
            .map(|branch| self.handle_branch(b, branch))
            .collect_vec();
        ThirStmt::mtch(scrut, branches, span)
    }

    fn expr_args(
        &mut self,
        b: &mut ThirBuilder,
        args: &HirConstructorArgs,
        stmts: &mut Vec<ThirStmt>,
    ) -> ThirConstructorArgs<ExprId> {
        match args {
            HirConstructorArgs::TupleLike(hir_exprs) => {
                ThirConstructorArgs::Tuple(
                    hir_exprs.iter().map(|e| self.expr(b, e, stmts)).collect(),
                )
            }
            HirConstructorArgs::StructLike { fields } => {
                ThirConstructorArgs::Struct(
                    fields
                        .iter()
                        .map(|f| (f.0, self.expr(b, &f.1, stmts)))
                        .collect(),
                )
            }
            HirConstructorArgs::None => ThirConstructorArgs::None,
        }
    }

    fn pat_args(
        &mut self,
        b: &mut ThirBuilder,
        arg: &HirPatternConstructorArgs,
    ) -> ThirConstructorArgs<ThirPattern> {
        match arg {
            HirPatternConstructorArgs::None => ThirConstructorArgs::None,
            HirPatternConstructorArgs::StructFields(pats) => {
                ThirConstructorArgs::Struct(
                    pats.iter()
                        .map(|pat| match pat {
                            HirStructFieldPattern::Name { id, name } => {
                                (*name, self.pattern_of_local(b, *id))
                            }
                            HirStructFieldPattern::Rebind { name, pattern } => {
                                (*name, self.pat(b, pattern))
                            }
                        })
                        .collect(),
                )
            }
            HirPatternConstructorArgs::TupleFields(hir_patterns) => {
                ThirConstructorArgs::Tuple(
                    hir_patterns.iter().map(|pat| self.pat(b, pat)).collect(),
                )
            }
        }
    }

    fn pattern_of_local(
        &mut self,
        b: &mut ThirBuilder,
        id: hir::LocalId,
    ) -> ThirPattern {
        let local_id = b.local_map[&id];
        let local = b.get_local(local_id);
        ThirPattern {
            kind: ThirPatternKind::Bind {
                local: local_id,
                mutable: false,
            },
            ty: local.ty,
            span: local.span,
        }
    }

    fn pat(&mut self, b: &mut ThirBuilder, pat: &HirPattern) -> ThirPattern {
        let ty = self
            .tc
            .pat_types(self.db)
            .get(&PatternId(pat.id))
            .copied()
            .unwrap_or(TypeRef::Unknown);
        let kind = match &pat.data {
            HirPatternDesc::Bind { id, mutable, .. } => {
                Some(ThirPatternKind::Bind {
                    local: b.local_map[id],
                    mutable: *mutable,
                })
            }
            HirPatternDesc::Any => Some(ThirPatternKind::Any),
            HirPatternDesc::Tuple(hir_patterns) => {
                Some(ThirPatternKind::Tuple(
                    hir_patterns.iter().map(|pat| self.pat(b, pat)).collect(),
                ))
            }
            HirPatternDesc::DestructureBinding { fields, .. } => {
                let def = ty.as_struct_ref(self.db);
                let fields = fields
                    .iter()
                    .map(|(symbol, hir_pattern)| {
                        (*symbol, {
                            match hir_pattern.as_ref() {
                                Either::Left(pat) => self.pat(b, pat),
                                Either::Right(id) => {
                                    self.pattern_of_local(b, *id)
                                }
                            }
                        })
                    })
                    .collect();
                def.map(|def| ThirPatternKind::Struct { def, fields })
            }
            HirPatternDesc::Constructor {
                resolution,
                name,
                fields,
            } => {
                let id = enum_item(self.db, resolution.interned())
                    .variants
                    .iter()
                    .position(|variant| variant.name == *name);
                if id.is_none() {
                    todo!(
                        "Error diagnostic for bad variant name in constructor patt()ern"
                    )
                }
                let args = self.pat_args(b, fields);
                let def = ty.as_enum_ref(self.db);
                match (id, def) {
                    (Some(id), Some(def)) => {
                        Some(ThirPatternKind::Constructor {
                            def,
                            idx: id,
                            args,
                        })
                    }
                    _ => None,
                }
            }
            HirPatternDesc::IntLit(lit) => Some(ThirPatternKind::IntLit(*lit)),
        };
        let kind = kind.unwrap_or(ThirPatternKind::Error);
        ThirPattern {
            kind,
            ty,
            span: pat.span,
        }
    }

    fn innermost_loop_scope(&self, b: &ThirBuilder) -> Option<ScopeId> {
        self.scope_stack
            .iter()
            .rev()
            .find(|k| b.get_scope(**k).kind == ScopeKind::Loop)
            .copied()
    }

    fn _destructure_pattern_init(
        &mut self,
        b: &mut ThirBuilder,
        pat: &HirPattern,
        value: ExprId,
        span: Span,
        v: &mut Vec<ThirStmt>,
    ) {
        match &pat.data {
            HirPatternDesc::Bind { id, .. } => {
                let thir_local = b.local_map[id];
                // Better span maybe
                v.push(ThirStmt::let_(thir_local, value, span))
            }
            HirPatternDesc::Any => {
                // Just compute the expression
                v.push(ThirStmt::expr(value, span));
            }
            HirPatternDesc::Tuple(hir_patterns) => {
                let ty = b.get_expr(value).ty;
                let the_tuple_local =
                    b.new_synthetic_local(ty, Mutability::Const, span);
                v.push(ThirStmt::let_(the_tuple_local, value, span));
                let the_tuple_place =
                    b.new_place(ThirPlace::local(the_tuple_local, b, span));
                for (idx, pat) in hir_patterns.iter().enumerate() {
                    let ty = self.tc.pat_types(self.db)[&PatternId(pat.id)];
                    let idx_place = b.with_synthetic_projection(
                        the_tuple_place,
                        Projection::TupleField(idx as u32, ty),
                        ty,
                    );
                    let idx_value =
                        b.new_expr(ThirExpr::use_place(idx_place, b, pat.span));
                    self._destructure_pattern_init(b, pat, idx_value, span, v);
                }
            }
            HirPatternDesc::DestructureBinding { fields, .. } => {
                let ty = b.get_expr(value).ty;
                let fresh =
                    b.new_synthetic_local(ty, Mutability::Const, pat.span);
                v.push(ThirStmt::let_(fresh, value, span));
                let place = b.new_place(ThirPlace::local(fresh, b, pat.span));
                let def =
                    ty.as_struct_ref(self.db).expect("TODO: handle bad case");
                for (name, binding) in fields {
                    let field_ty = def
                        .typeof_field(self.db, *name)
                        .unwrap_or(TypeRef::Error);
                    let field_place = b.with_synthetic_projection(
                        place,
                        Projection::Field(*name, field_ty),
                        field_ty,
                    );
                    let field_value =
                        b.new_expr(ThirExpr::use_place(field_place, b, span));
                    match binding {
                        Either::Left(pat) => {
                            self._destructure_pattern_init(
                                b,
                                pat,
                                field_value,
                                span,
                                v,
                            );
                        }
                        Either::Right(local) => {
                            let thir_local = b.local_map[local];
                            v.push(ThirStmt::let_(
                                thir_local,
                                field_value,
                                span,
                            ));
                        }
                    }
                }
            }
            HirPatternDesc::Constructor { .. } => todo!("Invalid lhs pattern"),
            HirPatternDesc::IntLit(_) => todo!("Invalid lhs pattern"),
        }
    }

    fn destructure_pattern_init(
        &mut self,
        b: &mut ThirBuilder,
        pat: &HirPattern,
        value: ExprId,
        span: Span,
    ) -> Vec<ThirStmt> {
        let mut v = vec![];
        self._destructure_pattern_init(b, pat, value, span, &mut v);
        v
    }

    fn expr(
        &mut self,
        b: &mut ThirBuilder,
        expr: &HirExpr,
        stmts: &mut Vec<ThirStmt>,
    ) -> ExprId {
        let ty = self.tc.expr_types(self.db)[&typecheck::ExprId(expr.id)];
        let kind = match &expr.data {
            HirExprDesc::IntLit(x) => ExprKind::IntLit(*x),
            HirExprDesc::CharLit(x) => ExprKind::Charlit(*x),
            HirExprDesc::StrLit(str_lit) => ExprKind::StrLit(*str_lit),
            HirExprDesc::CStrLit(str_lit) => ExprKind::CStrLit(*str_lit),
            HirExprDesc::BoolLit(b) => ExprKind::BoolLit(*b),
            HirExprDesc::Use(hir_place) => {
                let place = self.place(b, hir_place, stmts);
                ExprKind::Use(place)
            }
            HirExprDesc::AddressOf { place, mutability } => {
                let place = self.place(b, place, stmts);
                if ty.as_ref(self.db).is_some() {
                    ExprKind::Ref {
                        place,
                        mutability: *mutability,
                    }
                } else {
                    ExprKind::AddressOf {
                        place,
                        mutability: *mutability,
                    }
                }
            }
            HirExprDesc::Ref { place, mutability } => {
                let place = self.place(b, place, stmts);
                ExprKind::Ref {
                    place,
                    mutability: *mutability,
                }
            }
            HirExprDesc::UnresolvedCallDirect { .. } => ExprKind::Error,
            HirExprDesc::BinOp { lhs, op, rhs } => {
                let lhs = self.expr(b, lhs, stmts);
                let rhs = self.expr(b, rhs, stmts);
                ExprKind::BinOp { op: *op, lhs, rhs }
            }
            HirExprDesc::StructLit { fields, .. } => {
                let struct_def = ty.as_struct_ref(self.db).expect("TODO");
                let fields = fields
                    .iter()
                    .map(|field| (field.0, self.expr(b, &field.1, stmts)))
                    .collect_vec();
                ExprKind::StructLit { struct_def, fields }
            }
            HirExprDesc::Neg(hir_expr) => {
                let operand = self.expr(b, hir_expr, stmts);
                ExprKind::Neg(operand)
            }
            HirExprDesc::Not(hir_expr) => {
                let operand = self.expr(b, hir_expr, stmts);
                ExprKind::Not(operand)
            }
            HirExprDesc::Tuple(hir_exprs) => {
                let fields = hir_exprs
                    .iter()
                    .map(|e| self.expr(b, e, stmts))
                    .collect_vec();
                ExprKind::Tuple(fields)
            }
            HirExprDesc::SliceLit(hir_exprs) => {
                let exprs = hir_exprs
                    .iter()
                    .map(|e| self.expr(b, e, stmts))
                    .collect_vec();
                ExprKind::SliceLit(exprs)
            }
            HirExprDesc::SizeOf(partial_type_ref) => {
                ExprKind::SizeOf(partial_type_ref.plugged_by_unknown(self.db))
            }
            HirExprDesc::Constructor { name, args, .. } => {
                let enum_def = ty.as_enum_ref(self.db).expect("TODO");
                let idx = enum_item(self.db, enum_def.def.interned())
                    .variants
                    .iter()
                    .position(|v| v.name == *name)
                    .expect("TODO");
                let args = self.expr_args(b, args, stmts);
                ExprKind::Constructor {
                    enum_def,
                    idx,
                    args,
                }
            }
            HirExprDesc::CallDirect { target, args } => {
                let args = args
                    .iter()
                    .map(|arg| self.expr(b, arg, stmts))
                    .collect_vec();
                let call_infos =
                    &self.tc.call_infos(self.db)[&typecheck::ExprId(expr.id)];
                let fref = FunctionRef {
                    id: *target,
                    args: call_infos.substitution.clone(),
                    self_ty: None,
                    dispatch: Dispatch::Direct,
                };
                ExprKind::Call { called: fref, args }
            }
            HirExprDesc::CallMethod { receiver, args, .. } => {
                let call_infos =
                    &self.tc.call_infos(self.db)[&typecheck::ExprId(expr.id)];
                let thir_args = {
                    let mut thir_args = vec![self.expr(b, receiver, stmts)];
                    thir_args.extend(
                        args.iter().map(|arg| self.expr(b, arg, stmts)),
                    );
                    thir_args
                };
                let fref = FunctionRef {
                    id: call_infos.callee,
                    args: call_infos.substitution.clone(),
                    self_ty: None,
                    dispatch: Dispatch::Direct, // TODO: Compute this
                };
                ExprKind::Call {
                    called: fref,
                    args: thir_args,
                }
            }
            HirExprDesc::CallStatic { .. } => todo!(),
        };
        b.new_expr(ThirExpr {
            kind,
            ty,
            span: expr.span,
        })
    }

    fn auto_deref_place_if_needed(
        &mut self,
        b: &mut ThirBuilder,
        place: PlaceId,
    ) -> PlaceId {
        let (ty, span) = {
            let place = b.get_place(place);
            (place.ty, place.span)
        };
        let (proj, inner) = {
            if let Some((_, inner)) = ty.as_ref(self.db) {
                (Some(Projection::Deref), inner)
            } else {
                (None, ty)
            }
        };
        if let Some(proj) = proj {
            b.with_projection(place, proj, inner, span)
        } else {
            place
        }
    }

    fn expr_with_setup(
        &mut self,
        b: &mut ThirBuilder,
        expr: &HirExpr,
    ) -> ThirExprWithSetup {
        let mut stmts = vec![];
        let expr = self.expr(b, expr, &mut stmts);
        ThirExprWithSetup { stmts, expr }
    }

    fn place(
        &mut self,
        b: &mut ThirBuilder,
        place: &HirPlace,
        stmts: &mut Vec<ThirStmt>,
    ) -> PlaceId {
        match &place.kind {
            HirPlaceKind::Local(local_id) => {
                let thir_local = b.local_map[local_id];
                b.new_place(ThirPlace::local(thir_local, b, place.span))
            }
            HirPlaceKind::Field { base, field } => {
                let base = self.place(b, base, stmts);
                let place = self.auto_deref_place_if_needed(b, base);
                let (ty, span) = {
                    let place = b.get_place(place);
                    (place.ty, place.span)
                };
                let struct_ref = ty.as_struct_ref(self.db).expect("TODO");
                let field_ty =
                    struct_ref.typeof_field(self.db, *field).expect("TODO");
                b.with_projection(
                    place,
                    Projection::Field(*field, field_ty),
                    field_ty,
                    span,
                )
            }
            HirPlaceKind::TupleField { base, index } => {
                let base = self.place(b, base, stmts);
                let place = self.auto_deref_place_if_needed(b, base);
                let (ty, span) = {
                    let place = b.get_place(place);
                    (place.ty, place.span)
                };
                let mut tuple_ref = ty.as_tuple_ref(self.db).expect("TODO");
                let idx_ty =
                    tuple_ref.try_remove(*index as usize).expect("TODO");
                b.with_projection(
                    base,
                    Projection::TupleField(*index, idx_ty),
                    idx_ty,
                    span,
                )
            }
            HirPlaceKind::Deref(hir_place) => {
                let base = self.place(b, hir_place, stmts);
                let ty = b.get_place(base).ty;
                let type_id = ty.as_type_id().expect("TODO");
                let TypeDefId::Builtin(builtin) = type_id.def(self.db) else {
                    todo!()
                };
                if builtin.is_ptr_like(self.db).is_none() {
                    todo!("error diagnostic")
                }
                let deref_ty =
                    type_id.args(self.db).try_remove(0).expect("TODO");
                b.with_projection(base, Projection::Deref, deref_ty, place.span)
            }
            HirPlaceKind::Index { base, index } => {
                let base = self.place(b, base, stmts);
                let ty = b.get_place(base).ty;
                let type_id = ty.as_type_id().expect("TODO");
                let TypeDefId::Builtin(builtin) = type_id.def(self.db) else {
                    todo!()
                };
                if builtin.is_ptr_like(self.db).is_none() {
                    todo!("error diagnostic")
                }
                let deref_ty =
                    type_id.args(self.db).try_remove(0).expect("TODO");
                let index = self.expr(b, index, stmts);
                b.with_projection(
                    base,
                    Projection::Index(index),
                    deref_ty,
                    place.span,
                )
            }
            HirPlaceKind::Temporary(hir_expr) => {
                let value = self.expr(b, hir_expr, stmts);
                let (ty, span) = {
                    let expr = b.get_expr(value);
                    (expr.ty, expr.span)
                };
                let fresh = b.new_synthetic_local(ty, Mutability::Const, span);
                stmts.push(ThirStmt::let_(fresh, value, place.span));
                b.new_place(ThirPlace::local(fresh, b, span))
            }
        }
    }

    fn scoped<T>(
        &mut self,
        b: &mut ThirBuilder,
        span: Span,
        kind: ScopeKind,
        f: impl FnOnce(&mut Self, &mut ThirBuilder) -> T,
    ) -> (ScopeId, T) {
        let scope = self.push_scope(b, kind, span);
        let res = f(self, b);
        let popped = self.pop_scope();
        assert_eq!(Some(scope), popped);
        (scope, res)
    }
}

impl TypeRef {
    pub fn as_type_id(&self) -> Option<TypeId> {
        match self {
            TypeRef::Concrete(type_id) => Some(*type_id),
            _ => None,
        }
    }

    pub fn as_struct_ref(self, db: &dyn Db) -> Option<StructRef> {
        let type_id = self.as_type_id()?;
        match type_id.def(db) {
            TypeDefId::Struct(struct_id) => Some(StructRef {
                def: struct_id,
                args: type_id.args(db),
            }),
            _ => None,
        }
    }

    pub fn as_enum_ref(self, db: &dyn Db) -> Option<EnumRef> {
        let type_id = self.as_type_id()?;
        match type_id.def(db) {
            TypeDefId::Enum(enum_id) => Some(EnumRef {
                def: enum_id,
                args: type_id.args(db),
            }),
            _ => None,
        }
    }

    pub fn as_tuple_ref(self, db: &dyn Db) -> Option<Vec<Self>> {
        let type_id = self.as_type_id()?;
        if type_id.def(db) == TypeDefId::Builtin(BuiltinTypeId::tuple(db)) {
            Some(type_id.args(db))
        } else {
            None
        }
    }

    pub fn as_ref(self, db: &dyn Db) -> Option<(Mutability, TypeRef)> {
        let type_id = self.as_type_id()?;
        let def = type_id.def(db);
        if def == TypeDefId::Builtin(BuiltinTypeId::mut_ref(db)) {
            Some((Mutability::Mutable, *type_id.args(db).get(0)?))
        } else if def == TypeDefId::Builtin(BuiltinTypeId::ref_(db)) {
            Some((Mutability::Const, *type_id.args(db).get(0)?))
        } else {
            None
        }
    }
}

impl ThirPlace {
    pub fn local(local: LocalId, b: &ThirBuilder, span: Span) -> Self {
        let ty = b.get_local(local).ty;
        Self {
            base: PlaceBase::Local(local),
            projections: vec![],
            ty,
            span: span,
        }
    }
}

impl TypeRef {
    pub fn with_substitution(self, db: &dyn Db, sub: &[Self]) -> Self {
        match self {
            Self::Concrete(type_id) => Self::Concrete(TypeId::new(
                db,
                type_id.def(db),
                type_id
                    .args(db)
                    .iter()
                    .map(|t| t.with_substitution(db, sub))
                    .collect(),
            )),
            Self::Param(id) => sub[id.0],
            Self::Associated(_)
            | Self::Zelf
            | Self::Error
            | TypeRef::Unknown => self,
        }
    }
}

impl StructRef {
    pub fn typeof_field(&self, db: &dyn Db, field: Symbol) -> Option<TypeRef> {
        let item = struct_item(db, self.def.interned());
        let found = item.fields.iter().find(|f| f.name == field)?;
        let ctx = AstImplicitContext::new(
            db,
            ScopeOwnerId::Module(self.def.parent(db)),
            Arc::new([]),
        )
        .unwrap();
        Some(
            ctx.resolve(db, &found.ty.data)?
                .with_substitution(db, &self.args),
        )
    }
}

impl PartialTypeRef {
    pub fn plugged_by_unknown(&self, db: &dyn Db) -> TypeRef {
        match self {
            PartialTypeRef::Resolved(type_ref) => *type_ref,
            PartialTypeRef::WithHoles { def, args } => {
                TypeRef::Concrete(TypeId::new(
                    db,
                    *def,
                    args.iter()
                        .map(|arg| match arg {
                            hir::PartialTypeArg::Known(type_ref) => *type_ref,
                            hir::PartialTypeArg::Partial(partial) => {
                                partial.plugged_by_unknown(db)
                            }
                            hir::PartialTypeArg::Infer => TypeRef::Unknown,
                        })
                        .collect(),
                ))
            }
        }
    }
}
