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
    parse_tree::{
        Spanned,
        expr::{AstExpr, BinaryOperator},
        pattern::AstPattern,
        type_expr::AstAnyTypeExpr,
    },
    parser::ParseError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompoundAssignOp {
    Plus,
    Minus,
    Times,
    Div,
    Modulo,
}

impl CompoundAssignOp {
    pub fn to_binop(self) -> BinaryOperator {
        match self {
            Self::Plus => BinaryOperator::Plus,
            Self::Minus => BinaryOperator::Minus,
            Self::Times => BinaryOperator::Times,
            Self::Div => BinaryOperator::Div,
            Self::Modulo => BinaryOperator::Modulo,
        }
    }
}

pub type AstStmt = Spanned<AstStmtDesc>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AstMatchBranch {
    pub pat: AstPattern,
    pub guard: Option<AstExpr>,
    pub body: Box<AstStmt>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AstStmtDesc {
    Return { value: Option<AstExpr> },
    If { cond: AstExpr, then: Box<AstStmt>, else_: Option<Box<AstStmt>> },
    While { cond: AstExpr, body: Box<AstStmt> },
    For { element: AstPattern, iterator: AstExpr, body: Box<AstStmt> },
    LetDecl { pat: AstPattern, type_constraint: Option<AstAnyTypeExpr>, value: AstExpr },
    Block { stmts: Vec<AstStmt> },
    Assign { lhs: AstExpr, rhs: AstExpr },
    CompoundAssign { lhs: AstExpr, op: CompoundAssignOp, rhs: AstExpr },
    Match { scrutinee: AstExpr, branches: Vec<AstMatchBranch> },
    Break,
    Expr(AstExpr),
    Defer(Box<AstStmt>),
    Error(ParseError),
}
