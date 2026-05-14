use crate::parse_tree::{Spanned, expr::AstExpr, pattern::AstPattern, type_expr::AstAnyTypeExpr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompoundAssignOp {
    Plus,
    Minus,
    Times,
    Div,
    Modulo,
}

pub type AstStmt = Spanned<AstStmtDesc>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AstStmtDesc {
    Return {
        value: Option<AstExpr>,
    },
    If {
        cond: AstExpr,
        then: Box<AstStmt>,
        else_: Option<Box<AstStmt>>,
    },
    While {
        cond: AstExpr,
        body: Box<AstStmt>,
    },
    For {
        element: AstPattern,
        iterator: AstExpr,
        body: Box<AstStmt>,
    },
    LetDecl {
        pat: AstPattern,
        type_constraint: Option<AstAnyTypeExpr>,
        value: AstExpr,
    },
    Block {
        stmts: Vec<AstStmt>,
    },
    Assign {
        lhs: AstExpr,
        rhs: AstExpr,
    },
    CompoundAssign {
        lhs: AstExpr,
        op: CompoundAssignOp,
        rhs: AstExpr,
    },
    Expr(AstExpr),
}
