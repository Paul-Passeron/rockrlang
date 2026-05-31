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

use itertools::Itertools;
use la_arena::{Arena, Idx};

use crate::{
    Db,
    hir::{self, HirBody, HirStmt},
    thir::{
        ExprId, LocalId, PlaceId, ScopeId, Thir, ThirExpr, ThirLocal, ThirPlace, ThirScope,
        ThirStmt,
    },
    typecheck::TypeCheckResults,
};

struct ThirBuilder<'db> {
    db: &'db dyn Db,
    locals: Arena<ThirLocal>,
    exprs: Arena<ThirExpr>,
    places: Arena<ThirPlace>,
    scopes: Arena<ThirScope>,
    root: Vec<ThirStmt>,
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
            root: Vec::new(),
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

    pub fn stmt(&mut self, stmt: ThirStmt) {
        self.root.push(stmt);
    }

    pub fn finalize(self, params: Vec<LocalId>, zelf: Option<LocalId>) -> Thir {
        Thir {
            places: self.places,
            exprs: self.exprs,
            locals: self.locals,
            scopes: self.scopes,
            params,
            zelf,
            root: self.root,
        }
    }

    fn hir_local_to_thir(
        &mut self,
        locals: &Vec<hir::LocalInfo>,
        param: hir::LocalId,
        tc_results: &TypeCheckResults,
    ) -> LocalId {
        let infos = &locals[param.0 as usize];
        let local = ThirLocal {
            ty: tc_results.locals(self.db)[&param].unwrap_or(crate::ril::TypeRef::Error),
            mutability: infos.mutability,
            span: infos.span,
            source: Some((infos.id, infos.name)),
        };
        let res = self.new_local(local);
        self.local_map.insert(param, res);
        res
    }
}

fn handle_stmt<'db>(
    _b: &mut ThirBuilder,
    _hir: &'db HirBody<'db>,
    _tc_results: &'db TypeCheckResults,
    _stmt: &HirStmt,
) {
    todo!()
}

pub fn thir_body_from_hir<'db>(
    db: &'db dyn Db,
    hir: &'db HirBody<'db>,
    tc_results: &'db TypeCheckResults,
) -> Thir {
    let locals = hir.locals(db);
    let mut b = ThirBuilder::new(db);
    let params = hir
        .params(db)
        .iter()
        .map(|param| b.hir_local_to_thir(locals, *param, tc_results))
        .collect_vec();
    let zelf = hir
        .zelf(db)
        .map(|param| b.hir_local_to_thir(locals, param, tc_results));
    hir.stmts(db)
        .iter()
        .for_each(|stmt| handle_stmt(&mut b, hir, tc_results, stmt));
    b.finalize(params, zelf)
}
