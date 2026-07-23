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
    common::{
        location::Span,
        symbols::{StrLit, Symbol},
    },
    parse_tree::{
        Spanned,
        type_expr::{AstAnyTypeExpr, AstTypeExpr},
    },
};

pub type AstExpr = Spanned<AstExprDesc>;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum AstExprDesc {
    // Literals
    IntLit(i64),
    CharLit(char),
    StrLit(StrLit),
    CStrLit(StrLit),
    BoolLit(bool),

    // Names and resolution
    Name(Symbol),
    NameResolved {
        from: Spanned<Symbol>,
        to: Box<AstExpr>,
    },
    StaticCall {
        ty: AstTypeExpr,
        type_args: Vec<AstAnyTypeExpr>,
        method: Symbol,
        args: Vec<AstExpr>,
    },
    QualifiedPath {
        ty: AstTypeExpr,
        name: Symbol,
    },

    // Binary operations
    BinOp {
        lhs: Box<AstExpr>,
        op: BinaryOperator,
        rhs: Box<AstExpr>,
    },

    // Range  `a..b`
    Range {
        from: Box<AstExpr>,
        to: Box<AstExpr>,
    },

    Neg(Box<AstExpr>),
    Not(Box<AstExpr>),

    AddressOf(Box<AstExpr>),
    PostfixDeref(Box<AstExpr>),
    PrefixDeref(Box<AstExpr>),

    FieldAccess {
        object: Box<AstExpr>,
        field: Symbol,
    },
    TupleAccess {
        object: Box<AstExpr>,
        index: u32,
    },

    Call {
        callee: Box<AstExpr>,
        type_args: Vec<AstAnyTypeExpr>,
        args: Vec<AstExpr>,
    },
    MethodCall {
        object: Box<AstExpr>,
        method: Symbol,
        type_args: Vec<AstAnyTypeExpr>,
        args: Vec<AstExpr>,
    },

    Index {
        object: Box<AstExpr>,
        index: Box<AstExpr>,
    },

    StructLit {
        ty: AstTypeExpr,
        variant: Option<Symbol>,
        fields: Vec<AstStructField>,
    },

    Tuple(Vec<AstExpr>),

    SliceLit(Vec<AstExpr>),

    SizeOf(AstTypeExpr),
    TypeName(AstTypeExpr),
    /// @metadata(<expr>), retrives the metadata for the fat-pointer expr
    /// <expr>.
    Metadata(Box<AstExpr>),

    Ref(bool, Box<AstExpr>),

    As {
        expr: Box<AstExpr>,
        ty: AstTypeExpr,
    },
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct AstStructField {
    pub name: Symbol,
    pub name_span: Span,
    pub value: AstExpr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOperator {
    // Arithmetic
    Plus,
    Minus,
    Times,
    Div,
    Modulo,
    // Comparison
    Eq,
    Diff,
    Lt,
    Leq,
    Gt,
    Geq,
    // Logical
    And,
    Or,
    // Bitwise
    BitAnd,
    BitOr,
    BitXor,
}
