use crate::{
    common::symbols::{StrLit, Symbol},
    parse_tree::{Spanned, type_expr::AstTypeExpr},
};

pub type AstExpr = Spanned<AstExprDesc>;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum AstExprDesc {
    // Literals
    IntLit(i32),
    CharLit(char),
    StrLit(StrLit),
    CStrLit(StrLit),
    BoolLit(bool),

    // Names and resolution
    Name(Symbol),
    NameResolved {
        from: Symbol,
        to: Box<AstExpr>,
    },
    StaticCall {
        ty: AstTypeExpr,
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

    // Ref(Box<AstExpr>),
    Neg(Box<AstExpr>),
    Not(Box<AstExpr>),

    AddressOf(Box<AstExpr>),
    PostfixDeref(Box<AstExpr>),

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
        args: Vec<AstExpr>,
    },
    MethodCall {
        object: Box<AstExpr>,
        method: Symbol,
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
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct AstStructField {
    pub name: Symbol,
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
