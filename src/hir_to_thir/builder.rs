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

use crate::{
    Db,
    common::{
        arena::{Arena, Idx},
        location::Span,
    },
    hir::{self, LocalInfo, Mutability},
    resolved::{FunctionId, TypeRef},
    thir::{
        ExprId, LocalId, PlaceId, Projection, ScopeId, Thir, ThirExpr, ThirLocal,
        ThirPlace, ThirScope, stmt::ThirStmt,
    },
    typecheck::TypeCheckResults,
};

pub struct ThirBuilder<'db> {
    db: &'db dyn Db,
    locals: Arena<ThirLocal>,
    pub(super) exprs: Arena<ThirExpr>,
    pub(super) places: Arena<ThirPlace>,
    scopes: Arena<ThirScope>,
    pub(super) local_map: HashMap<hir::LocalId, LocalId>,
}

impl<'db> ThirBuilder<'db> {
    pub(super) fn new(db: &'db dyn Db) -> Self {
        Self {
            db,
            locals: Arena::new(),
            exprs: Arena::new(),
            places: Arena::new(),
            scopes: Arena::new(),
            local_map: HashMap::new(),
        }
    }

    pub fn new_local(&self, local: ThirLocal) -> LocalId {
        self.locals.insert(local)
    }
    pub fn new_expr(&self, expr: ThirExpr) -> ExprId {
        self.exprs.insert(expr)
    }
    pub fn new_place(&self, place: ThirPlace) -> PlaceId {
        self.places.insert(place)
    }
    pub fn new_scope(&self, scope: ThirScope) -> ScopeId {
        self.scopes.insert(scope)
    }

    pub fn set_scope_drops(&mut self, id: ScopeId, drops: Vec<LocalId>) {
        self.scopes.get_shared_mut(id).drops = drops;
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
        root_scope: ScopeId,
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
            root_scope,
        }
    }

    pub(super) fn register_hir_local(
        &mut self,
        infos: &LocalInfo,
        tc_results: TypeCheckResults<'_>,
    ) {
        let local = ThirLocal {
            ty: tc_results.locals(self.db)[&infos.id].unwrap_or(TypeRef::Unknown),
            mutability: infos.mutability,
            span: infos.span,
            source: Some((infos.id, infos.name)),
            is_synthetic: infos.is_synthetic,
        };
        let res = self.new_local(local);
        self.local_map.insert(infos.id, res);
    }

    pub(super) fn with_synthetic_projection(
        &self,
        place: PlaceId,
        proj: Projection,
        ty: TypeRef,
    ) -> PlaceId {
        let mut place = self.get_place(place).clone();
        place.projections.push(proj);
        place.ty = ty;
        self.new_place(place)
    }

    pub(super) fn with_projection(
        &self,
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

    pub(super) fn new_synthetic_local(
        &self,
        ty: TypeRef,
        mutability: Mutability,
        span: Span,
    ) -> LocalId {
        self.new_local(ThirLocal {
            ty,
            mutability,
            span,
            source: None,
            is_synthetic: true,
        })
    }
}
