use crate::{
    common::{
        location::Span,
        symbols::{StrLit, Symbol},
    },
    parse_tree::expr::BinaryOperator,
    ril::{FunctionId, TypeId, TypeRef},
};

pub struct RilExpr {
    pub ty: TypeRef,
    pub desc: RilExprDesc,
    pub span: Span,
}

pub struct RilStmt {
    pub desc: RilStmtDesc,
    pub span: Span,
}

pub enum RilStmtDesc {
    Expr(RilExpr),
    Return {
        value: Option<RilExpr>,
    },
    If {
        cond: RilExpr,
        then_: Box<RilStmt>,
        else_: Option<Box<RilStmt>>,
    },
    While {
        cond: RilExpr,
        body: Box<RilStmt>,
    },
    For {
        element: RilPattern,
        iterator: RilExpr,
        body: Box<RilStmt>,
    },
    Block {
        stmts: Vec<RilStmt>,
    },
    // CompoundAssign is desugared into the proper Assign
    Assign {
        lhs: RilExpr,
        rhs: RilExpr,
    },
    LetDecl(RilLetDecl),
}

pub enum RilReceiver {
    Static(TypeRef),
    Object(Box<RilExpr>),
}

pub enum RilExprDesc {
    IntLit(i32),
    CharLit(char),
    StrLit(StrLit),
    BoolLit(bool),
    Name {
        symbol: Symbol,
        local_id: LocalId,
    },
    FieldAccess {
        object: Box<RilExpr>,
        field: Symbol,
    },
    TupleAccess {
        object: Box<RilExpr>,
        index: u32,
    },
    MethodCall {
        receiver: Box<RilExpr>,
        method: FunctionId,
        args: Vec<RilExpr>,
    },
    BinOp {
        lhs: Box<RilExpr>,
        op: BinaryOperator,
        rhs: Box<RilExpr>,
    },
    Ref(Box<RilExpr>),
    Neg(Box<RilExpr>),
    Not(Box<RilExpr>),
    AddressOf(Box<RilExpr>),
}

pub struct LocalId(pub usize);

pub enum RilPattern {
    Bind { id: LocalId, name: Symbol },
    Any,
    Tuple(Vec<RilPattern>),
    Constructor { id: TypeId, fields: Vec<RilPattern> },
}

pub struct RilLetDecl {
    pub pattern: RilPattern,
    pub bindings: Vec<LocalId>,
    pub value: RilExpr,
}
