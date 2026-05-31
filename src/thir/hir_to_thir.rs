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

use std::{collections::HashMap, thread::Scope};

use itertools::Itertools;
use la_arena::{Arena, Idx};

use crate::{
    Db,
    common::location::Span,
    hir::{self, HirBody, HirExpr, HirPattern, HirStmt, HirStmtKind},
    thir::{
        ExprId, LocalId, PlaceId, ScopeId, Thir, ThirExpr, ThirLocal, ThirPlace, ThirScope,
        stmt::ThirStmt,
    },
    typecheck::TypeCheckResults,
};

use super::ScopeKind;

struct ThirBuilder<'db> {
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

    fn hir_local_to_thir(
        &mut self,
        locals: &Vec<hir::LocalInfo>,
        param: hir::LocalId,
        tc_results: TypeCheckResults<'_>,
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
        let locals = self.hir.locals(self.db);
        let mut b = ThirBuilder::new(self.db);
        let params = self
            .hir
            .params(self.db)
            .iter()
            .map(|param| b.hir_local_to_thir(locals, *param, self.tc))
            .collect_vec();
        let zelf = self
            .hir
            .zelf(self.db)
            .map(|param| b.hir_local_to_thir(locals, param, self.tc));
        let stmts = self
            .hir
            .stmts(self.db)
            .iter()
            .flat_map(|stmt| self.handle_stmt(&mut b, stmt))
            .collect_vec();
        b.finalize(params, zelf, stmts)
    }

    fn innermost_loop_scope(&self) -> Option<ScopeId> {
        todo!()
    }

    fn handle_break(&self, span: Span) -> ThirStmt {
        match self.innermost_loop_scope() {
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

    fn destructure_pattern_init(
        &mut self,
        b: &mut ThirBuilder,
        pat: &HirPattern,
        value: ExprId,
    ) -> Vec<ThirStmt> {
        todo!()
    }

    fn expr(&mut self, b: &mut ThirBuilder, expr: &HirExpr) -> ExprId {
        todo!()
    }

    fn handle_block(
        &mut self,
        b: &mut ThirBuilder,
        stmts: &Vec<HirStmt>,
        span: Span,
    ) -> Vec<ThirStmt> {
        let scope = b.new_scope(ThirScope {
            kind: ScopeKind::Block,
            span,
        });
        self.scope_stack.push(scope);
        todo!()
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
            } => todo!(),
            HirStmtKind::Assign { lhs, rhs } => todo!(),
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
            HirStmtKind::Block(hir_stmts) => self.handle_block(b, hir_stmts, stmt.span),
            HirStmtKind::Defer(_) => todo!("error diagnostic for unimplemented defer stmts"),
            HirStmtKind::Break => vec![self.handle_break(stmt.span)],
        }
    }

    fn push_scope(&mut self, b: &mut ThirBuilder, kind: ScopeKind, span: Span) -> ScopeId {
        let scope_id = b.new_scope(ThirScope { kind, span });
        self.scope_stack.push(scope_id);
        scope_id
    }

    fn pop_scope(&mut self, b: &mut ThirBuilder) -> Option<ScopeId> {
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
        let then_scope = self.push_scope(b, ScopeKind::Block, then.span);
        let then_stmts = self.handle_stmt(b, then);
        let popped = self.pop_scope(b);
        assert_eq!(Some(then_scope), popped);
        let (else_stmts, else_scope) = match else_ {
            Some(stmt) => {
                let else_scope = self.push_scope(b, ScopeKind::Block, stmt.span);
                let else_stmts = self.handle_stmt(b, stmt);
                let popped = self.pop_scope(b);
                assert_eq!(Some(else_scope), popped);
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
        let scope = self.push_scope(b, ScopeKind::Loop, span);
        let body = self.handle_stmt(b, body);
        let popped = self.pop_scope(b);
        assert_eq!(Some(scope), popped);
        ThirStmt::whl(cond, scope, body, span)
    }
}
