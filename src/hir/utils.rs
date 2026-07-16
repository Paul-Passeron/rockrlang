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
    common::{location::Span, symbols::Symbol},
    hir::{
        HirExpr, HirExprDesc, HirMatchBranch, HirPattern, HirPatternConstructorArgs,
        HirPatternDesc, HirPlace, HirPlaceKind, HirStmt, HirStmtKind, LocalId,
        lower_fundef::LowerFundef,
    },
    name_resolve::interfaces::core_opt_enum,
};

impl LowerFundef<'_> {
    pub fn new_expr(&self, desc: HirExprDesc, span: Span) -> HirExpr {
        HirExpr { id: self.alloc.fresh(), data: desc, span }
    }

    pub fn new_place(&self, kind: HirPlaceKind, span: Span) -> HirPlace {
        HirPlace { id: self.alloc.fresh(), kind, span }
    }

    pub fn new_pattern(&self, desc: HirPatternDesc, span: Span) -> HirPattern {
        HirPattern { id: self.alloc.fresh(), data: desc, span }
    }

    pub fn new_stmt(&self, kind: HirStmtKind, span: Span) -> HirStmt {
        HirStmt { id: self.alloc.fresh(), kind, span }
    }

    /// Returns the pattern Some(<pat>)
    /// Use or code synthesis so the span is the same as <pat>.
    pub fn wrap_some(&self, pat: HirPattern) -> HirPattern {
        let option_enum = core_opt_enum(self.db);
        let span = pat.span;
        self.new_pattern(
            HirPatternDesc::Constructor {
                resolution: option_enum,
                name: Symbol::new(self.db, "Some"),
                fields: HirPatternConstructorArgs::TupleFields(vec![pat]),
            },
            span,
        )
    }

    pub fn while_true_do(&self, stmt: HirStmt, cond_span: Span) -> HirStmt {
        let stmt_span = stmt.span;
        self.new_stmt(
            HirStmtKind::While {
                cond: self.new_expr(HirExprDesc::BoolLit(true), cond_span),
                body: stmt.boxed(),
            },
            stmt_span,
        )
    }

    /// match <expr> { Some(pat) => <stmt>, _ => { break; }}
    pub fn match_some_do_or_break(
        &self,
        expr: HirExpr,
        pat: HirPattern,
        locals: Vec<LocalId>,
        stmt: HirStmt,
    ) -> HirStmt {
        let stmt_span = stmt.span;
        let pat_span = pat.span;

        self.new_stmt(
            HirStmtKind::Match {
                scrutinee: expr,
                branches: vec![
                    HirMatchBranch {
                        pattern: self.wrap_some(pat),
                        locals,
                        guard: None,
                        body: stmt.boxed(),
                    },
                    HirMatchBranch {
                        pattern: self.new_pattern(HirPatternDesc::Any, pat_span),
                        locals: vec![],
                        guard: None,
                        body: self.new_stmt(HirStmtKind::Break, pat_span).boxed(),
                    },
                ],
            },
            stmt_span,
        )
    }

    pub fn declare_single_var(
        &self,
        var_id: LocalId,
        init: HirExpr,
        span: Span,
    ) -> HirStmt {
        let var_name = self.locals[var_id].name;
        self.new_stmt(
            HirStmtKind::Let {
                pattern: self.new_pattern(
                    HirPatternDesc::Bind { id: var_id, name: var_name, mutable: true },
                    span,
                ),
                locals: vec![var_id],
                ty_annotation: None,
                init,
            },
            span,
        )
    }
}

impl HirExpr {
    pub fn boxed(self) -> Box<Self> {
        Box::new(self)
    }
}

impl HirPattern {
    pub fn boxed(self) -> Box<Self> {
        Box::new(self)
    }
}

impl HirPlace {
    pub fn boxed(self) -> Box<Self> {
        Box::new(self)
    }
}

impl HirStmt {
    pub fn boxed(self) -> Box<Self> {
        Box::new(self)
    }
}
