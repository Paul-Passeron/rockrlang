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

    // Range  `a..b`
    Range {
        from: Box<Expr>,
        to: Box<Expr>,
    },

    Ref(Box<Expr>),
    Neg(Box<Expr>),
    Not(Box<Expr>),

    AddressOf(Box<Expr>),
    PostfixDeref(Box<Expr>),

    FieldAccess {
        object: Box<Expr>,
        field: Symbol,
    },
    TupleAccess {
        object: Box<Expr>,
        index: u32,
    },
    ArrowAccess {
        object: Box<Expr>,
        field: Symbol,
    },

    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    MethodCall {
        object: Box<Expr>,
        method: Symbol,
        args: Vec<Expr>,
    },

    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },

    StructLit {
        ty: TypeExpr,
        fields: Vec<StructField>,
    },

    Tuple(Vec<Expr>),

    SliceLit(Vec<Expr>),

    SizeOf(TypeExpr),
}

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
