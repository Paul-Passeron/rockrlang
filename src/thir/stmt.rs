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

use crate::{
    common::location::Span,
    thir::{
        ExprId, LocalId, PlaceId, ScopeId, ThirExprWithSetup, ThirMatchBranch,
    },
};

pub struct ThirStmt {
    pub kind: StmtKind,
    pub span: Span,
}

pub enum StmtKind {
    Block {
        scope: ScopeId,
        stmts: Vec<ThirStmt>,
    },
    If {
        cond: ThirExprWithSetup,
        then: Vec<ThirStmt>,
        then_scope: ScopeId,
        else_: Option<Vec<ThirStmt>>,
        else_scope: Option<ScopeId>,
    },
    While {
        scope: ScopeId,
        cond: ThirExprWithSetup,
        body: Vec<ThirStmt>,
    },
    Let {
        local: LocalId,
        init: ExprId,
    },
    Assign {
        place: PlaceId,
        rhs: ExprId,
    },
    Return(Option<ExprId>),
    Break(ScopeId),
    Continue(ScopeId),
    Match {
        scrutinee: ThirExprWithSetup,
        branches: Vec<ThirMatchBranch>,
    },
    Expr(ExprId),
    Error,
}

impl ThirStmt {
    pub fn brk(id: ScopeId, span: Span) -> Self {
        Self { kind: StmtKind::Break(id), span }
    }

    pub fn error(span: Span) -> Self {
        Self { kind: StmtKind::Error, span }
    }

    pub fn ifte(
        cond: ThirExprWithSetup,
        then: Vec<ThirStmt>,
        then_scope: ScopeId,
        else_: Option<Vec<ThirStmt>>,
        else_scope: Option<ScopeId>,
        span: Span,
    ) -> Self {
        Self {
            kind: StmtKind::If { cond, then, then_scope, else_, else_scope },
            span,
        }
    }

    pub fn expr(expr: ExprId, span: Span) -> Self {
        Self { kind: StmtKind::Expr(expr), span }
    }

    pub fn whl(
        cond: ThirExprWithSetup,
        scope: ScopeId,
        body: Vec<Self>,
        span: Span,
    ) -> Self {
        Self { kind: StmtKind::While { scope, cond, body }, span }
    }

    pub fn ret(expr: Option<ExprId>, span: Span) -> Self {
        Self { kind: StmtKind::Return(expr), span }
    }

    pub fn mtch(
        scrut: ThirExprWithSetup,
        branches: Vec<ThirMatchBranch>,
        span: Span,
    ) -> Self {
        Self { kind: StmtKind::Match { scrutinee: scrut, branches }, span }
    }

    pub fn block(scope: ScopeId, stmts: Vec<Self>, span: Span) -> Self {
        Self { kind: StmtKind::Block { scope, stmts }, span }
    }

    pub fn assign(place: PlaceId, expr: ExprId, span: Span) -> Self {
        Self { kind: StmtKind::Assign { place, rhs: expr }, span }
    }

    pub fn let_(local: LocalId, value: ExprId, span: Span) -> Self {
        Self { kind: StmtKind::Let { local, init: value }, span }
    }
}
