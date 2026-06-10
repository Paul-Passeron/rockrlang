use crate::{
    common::location::Span,
    hir::{
        HirExpr, HirExprDesc, HirPattern, HirPatternDesc, HirPlace, HirPlaceKind,
        HirStmt, HirStmtKind, lower_fundef::LowerFundef,
    },
};

impl LowerFundef<'_> {
    pub fn new_expr(&self, desc: HirExprDesc, span: Span) -> HirExpr {
        HirExpr {
            id: self.alloc.fresh(),
            data: desc,
            span,
        }
    }

    pub fn new_place(&self, kind: HirPlaceKind, span: Span) -> HirPlace {
        HirPlace {
            id: self.alloc.fresh(),
            kind,
            span,
        }
    }

    pub fn new_pattern(&self, desc: HirPatternDesc, span: Span) -> HirPattern {
        HirPattern {
            id: self.alloc.fresh(),
            data: desc,
            span,
        }
    }

    pub fn new_stmt(&self, kind: HirStmtKind, span: Span) -> HirStmt {
        HirStmt {
            id: self.alloc.fresh(),
            kind,
            span,
        }
    }
}
