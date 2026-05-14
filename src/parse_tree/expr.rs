use crate::{
    common::symbols::{StrLit, Symbol},
    parse_tree::{Spanned, type_expr::TypeExpr},
};

pub type Expr = Spanned<ExprDesc>;

#[derive(PartialEq, Eq, Hash, Debug)]
pub enum ExprDesc {
    // Literals
    IntLit(i32),
    CharLit(char),
    StrLit(StrLit),
    BoolLit(bool),

    // Names and resolution
    Name(Symbol),
    /// `A::B::expr` — module / namespace resolution
    NameResolved {
        from: Symbol,
        to: Box<Expr>,
    },
    /// `Type<T>::method(args)` — static method call on a generic type
    StaticCall {
        ty: TypeExpr,
        method: Symbol,
        args: Vec<Expr>,
    },

    // Binary operations
    BinOp {
        lhs: Box<Expr>,
        op: BinaryOperator,
        rhs: Box<Expr>,
    },

    // Assignment and compound assignment
    Assign {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    CompoundAssign {
        lhs: Box<Expr>,
        op: CompoundAssignOp,
        rhs: Box<Expr>,
    },

    // Range  `a..b`
    Range {
        from: Box<Expr>,
        to: Box<Expr>,
    },

    // Prefix unary operators
    /// `*expr`  — pointer dereference (prefix)
    Deref(Box<Expr>),
    /// `&expr`  — reference / address-of (prefix)
    Ref(Box<Expr>),
    /// `-expr`  — arithmetic negation
    Neg(Box<Expr>),
    /// `!expr`  — boolean / bitwise not
    Not(Box<Expr>),

    // Postfix operators
    /// `expr@`  — take address (postfix)
    AddressOf(Box<Expr>),
    /// `expr$`  — dereference (postfix)
    PostfixDeref(Box<Expr>),

    // Field / pointer-field access
    /// `expr.field`
    FieldAccess {
        object: Box<Expr>,
        field: Symbol,
    },
    /// `expr->field`
    ArrowAccess {
        object: Box<Expr>,
        field: Symbol,
    },

    // Calls
    /// `expr(args)`  — free function call or closure call
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    /// `expr.method(args)`  — method call
    MethodCall {
        object: Box<Expr>,
        method: Symbol,
        args: Vec<Expr>,
    },

    // Indexing  `expr[idx]`
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },

    // Struct literal  `Type { .field = expr, ... }`
    StructLit {
        ty: TypeExpr,
        fields: Vec<StructField>,
    },

    // Parenthesised expression
    Paren(Box<Expr>),
}

/// A single field initialiser inside a struct literal: `.name = expr`
#[derive(PartialEq, Eq, Hash, Debug)]
pub struct StructField {
    pub name: Symbol,
    pub value: Expr,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompoundAssignOp {
    Plus,
    Minus,
    Times,
    Div,
    Modulo,
}
