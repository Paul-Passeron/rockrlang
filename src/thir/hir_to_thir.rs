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

use std::{collections::HashMap, ops::Index};

use itertools::{Either, Itertools};
use la_arena::{Arena, Idx};

use crate::{
    Db,
    common::{location::Span, symbols::Symbol},
    hir::{
        self, HirBody, HirExpr, HirMatchBranch, HirPattern, HirPatternConstructorArgs,
        HirPatternDesc, HirPlace, HirStmt, HirStmtKind, HirStructFieldPattern, LocalInfo,
        Mutability,
    },
    name_resolve::type_expr::enum_item,
    ril::{TypeDefId, TypeId, TypeRef},
    thir::{
        EnumRef, ExprId, LocalId, PlaceBase, PlaceId, Projection, ScopeId, StructRef, Thir,
        ThirConstructorArgs, ThirExpr, ThirLocal, ThirMatchBranch, ThirPattern, ThirPlace,
        ThirScope, stmt::ThirStmt,
    },
    typecheck::{PatternId, TypeCheckResults},
};

use super::{ScopeKind, ThirPatternKind};

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
        params: Vec<LocalId>,
        zelf: Option<LocalId>,
        stmts: Vec<ThirStmt>,
    ) -> Thir {
        Thir {
            places: self.places,
            exprs: self.exprs,
            locals: self.locals,
            scopes: self.scopes,
            params,
            zelf,
            root: stmts,
        }
    }

    fn register_hir_local(&mut self, infos: &LocalInfo, tc_results: TypeCheckResults<'_>) {
        let local = ThirLocal {
            ty: tc_results.locals(self.db)[&infos.id].unwrap_or(crate::ril::TypeRef::Error),
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

    fn new_synthetic_local(&mut self, ty: TypeRef, mutability: Mutability, span: Span) -> LocalId {
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
    pub fn new(db: &'db dyn Db, hir: HirBody<'db>, tc: TypeCheckResults<'db>) -> Self {
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
        b.finalize(params, zelf, stmts)
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

    fn handle_block(&mut self, b: &mut ThirBuilder, stmts: &Vec<HirStmt>, span: Span) -> ThirStmt {
        let (scope, stmts) = self.scoped(b, span, ScopeKind::Block, |this, b| {
            stmts
                .iter()
                .flat_map(|stmt| this.handle_stmt(b, stmt))
                .collect_vec()
        });
        ThirStmt::block(scope, stmts, span)
    }

    fn handle_stmt(&mut self, b: &mut ThirBuilder, stmt: &HirStmt) -> Vec<ThirStmt> {
        match &stmt.kind {
            HirStmtKind::Let { pattern, init, .. } => {
                let value = self.expr(b, init);
                self.destructure_pattern_init(b, pattern, value)
            }
            HirStmtKind::Match {
                scrutinee,
                branches,
            } => {
                vec![self.handle_match(b, scrutinee, branches, stmt.span)]
            }
            HirStmtKind::Assign { lhs, rhs } => {
                let place = self.place(b, lhs);
                let value = self.expr(b, rhs);
                vec![ThirStmt::assign(place, value, stmt.span)]
            }
            HirStmtKind::Expr(hir_expr) => {
                let expr = self.expr(b, hir_expr);
                vec![ThirStmt::expr(expr, hir_expr.span)]
            }
            HirStmtKind::Return(hir_expr) => {
                let expr = hir_expr.as_ref().map(|expr| self.expr(b, expr));
                vec![ThirStmt::ret(expr, stmt.span)]
            }
            HirStmtKind::If { cond, then, else_ } => vec![self.handle_if_block(
                b,
                cond,
                then,
                else_.as_ref().map(Box::as_ref),
                stmt.span,
            )],
            HirStmtKind::While { cond, body } => {
                vec![self.handle_while(b, cond, body, stmt.span)]
            }
            HirStmtKind::Block(hir_stmts) => vec![self.handle_block(b, hir_stmts, stmt.span)],
            HirStmtKind::Defer(_) => todo!("error diagnostic for unimplemented defer stmts"),
            HirStmtKind::Break => vec![self.handle_break(b, stmt.span)],
        }
    }

    fn push_scope(&mut self, b: &mut ThirBuilder, kind: ScopeKind, span: Span) -> ScopeId {
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
        let thir_cond = self.expr(b, cond);

        let (then_scope, then_stmts) = self.scoped(b, then.span, ScopeKind::Block, |this, b| {
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
        let cond = self.expr(b, cond);
        let (scope, body) = self.scoped(b, span, ScopeKind::Loop, |this, b| {
            this.handle_stmt(b, body)
        });
        ThirStmt::whl(cond, scope, body, span)
    }

    fn handle_branch(&mut self, b: &mut ThirBuilder, branch: &HirMatchBranch) -> ThirMatchBranch {
        let pattern = self.pat(b, &branch.pattern);
        let guard = branch.guard.as_ref().map(|guard| self.expr(b, guard));
        let (body_scope, body) = self.scoped(b, branch.body.span, ScopeKind::Block, |this, b| {
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
        let scrut = self.expr(b, scrutinee);
        let branches = branches
            .iter()
            .map(|branch| self.handle_branch(b, branch))
            .collect_vec();
        ThirStmt::mtch(scrut, branches, span)
    }

    fn pat_args(
        &mut self,
        b: &mut ThirBuilder,
        arg: &HirPatternConstructorArgs,
    ) -> ThirConstructorArgs<ThirPattern> {
        match arg {
            HirPatternConstructorArgs::None => ThirConstructorArgs::None,
            HirPatternConstructorArgs::StructFields(pats) => ThirConstructorArgs::Struct(
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
            ),
            HirPatternConstructorArgs::TupleFields(hir_patterns) => ThirConstructorArgs::Tuple(
                hir_patterns.iter().map(|pat| self.pat(b, pat)).collect(),
            ),
        }
    }

    fn pattern_of_local(&mut self, b: &mut ThirBuilder, id: hir::LocalId) -> ThirPattern {
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
            .unwrap_or(TypeRef::Error);
        let kind = match &pat.data {
            HirPatternDesc::Bind { id, mutable, .. } => Some(ThirPatternKind::Bind {
                local: b.local_map[id],
                mutable: *mutable,
            }),
            HirPatternDesc::Any => Some(ThirPatternKind::Any),
            HirPatternDesc::Tuple(hir_patterns) => Some(ThirPatternKind::Tuple(
                hir_patterns.iter().map(|pat| self.pat(b, pat)).collect(),
            )),
            HirPatternDesc::DestructureBinding { fields, .. } => {
                let def = ty.as_struct_ref(self.db);
                let fields = fields
                    .iter()
                    .map(|(symbol, hir_pattern)| {
                        (*symbol, {
                            match hir_pattern.as_ref() {
                                Either::Left(pat) => self.pat(b, pat),
                                Either::Right(id) => self.pattern_of_local(b, *id),
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
                    todo!("Error diagnostic for bad variant name in constructor patt()ern")
                }
                let args = self.pat_args(b, fields);
                let def = ty.as_enum_ref(self.db);
                match (id, def) {
                    (Some(id), Some(def)) => {
                        Some(ThirPatternKind::Constructor { def, idx: id, args })
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

    fn place(&mut self, b: &mut ThirBuilder, place: &HirPlace) -> PlaceId {
        todo!()
    }

    fn _destructure_pattern_init(
        &mut self,
        b: &mut ThirBuilder,
        pat: &HirPattern,
        value: ExprId,
        v: &mut Vec<ThirStmt>,
    ) {
        match &pat.data {
            HirPatternDesc::Bind { id, .. } => {
                let thir_local = b.local_map[id];
                // Better span maybe
                v.push(ThirStmt::let_(thir_local, value, pat.span))
            }
            HirPatternDesc::Any => {
                // Just compute the expression
                v.push(ThirStmt::expr(value, b.get_expr(value).span));
            }
            HirPatternDesc::Tuple(hir_patterns) => {
                let (ty, span) = {
                    let expr = b.get_expr(value);
                    (expr.ty, expr.span)
                };
                let the_tuple_local = b.new_synthetic_local(ty, Mutability::Const, span);
                v.push(ThirStmt::let_(the_tuple_local, value, span));
                let the_tuple_place = b.new_place(ThirPlace::local(the_tuple_local, b, span));
                for (idx, pat) in hir_patterns.iter().enumerate() {
                    let ty = self.tc.pat_types(self.db)[&PatternId(pat.id)];
                    let idx_place = b.with_synthetic_projection(
                        the_tuple_place,
                        Projection::TupleField(idx as u32, ty),
                        ty,
                    );
                    let idx_value = b.new_expr(ThirExpr::use_place(idx_place, b, pat.span));
                    self._destructure_pattern_init(b, pat, idx_value, v);
                }
            }
            HirPatternDesc::DestructureBinding { fields, .. } => {
                let (ty, span) = {
                    let expr = b.get_expr(value);
                    (expr.ty, expr.span)
                };
                let fresh = b.new_synthetic_local(ty, Mutability::Const, pat.span);
                let whole_span = pat.span.start().span(span.end());
                v.push(ThirStmt::let_(fresh, value, whole_span));
                let place = b.new_place(ThirPlace::local(fresh, b, pat.span));
                let def = ty.as_struct_ref(self.db).expect("TODO: handle bad case");
                for (name, binding) in fields {
                    let field_ty = def.typeof_field(*name).unwrap_or(TypeRef::Error);
                    let field_place = b.with_synthetic_projection(
                        place,
                        Projection::Field(*name, field_ty),
                        field_ty,
                    );
                    let field_value = b.new_expr(ThirExpr::use_place(field_place, b, span));
                    match binding {
                        Either::Left(pat) => {
                            self._destructure_pattern_init(b, pat, field_value, v);
                        }
                        Either::Right(local) => {
                            let thir_local = b.local_map[local];
                            v.push(ThirStmt::let_(thir_local, field_value, whole_span));
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
    ) -> Vec<ThirStmt> {
        let mut v = vec![];
        self._destructure_pattern_init(b, pat, value, &mut v);
        v
    }

    fn expr(&mut self, b: &mut ThirBuilder, expr: &HirExpr) -> ExprId {
        todo!()
    }

    fn scoped<T>(
        &mut self,
        b: &mut ThirBuilder,
        span: Span,
        kind: ScopeKind,
        f: impl Fn(&mut Self, &mut ThirBuilder) -> T,
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

    pub fn as_struct_ref(&self, db: &dyn Db) -> Option<StructRef> {
        let type_id = self.as_type_id()?;
        match type_id.def(db) {
            TypeDefId::Struct(struct_id) => Some(StructRef {
                def: struct_id,
                args: type_id.args(db),
            }),
            _ => None,
        }
    }

    pub fn as_enum_ref(&self, db: &dyn Db) -> Option<EnumRef> {
        let type_id = self.as_type_id()?;
        match type_id.def(db) {
            TypeDefId::Enum(enum_id) => Some(EnumRef {
                def: enum_id,
                args: type_id.args(db),
            }),
            _ => None,
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

impl StructRef {
    pub fn typeof_field(&self, field: Symbol) -> Option<TypeRef> {
        todo!()
    }
}
