use crate::parse_tree::{Spanned, expr::Expr, pattern::Pattern, type_expr::AnyTypeExpr};

pub type Stmt = Spanned<StmtDesc>;

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum StmtDesc {
    Return {
        value: Option<Expr>,
    },
    If {
        cond: Expr,
        then: Box<Stmt>,
        else_: Option<Box<Stmt>>,
    },
    For {
        element: Pattern,
        iterator: Expr,
        body: Box<Stmt>,
    },
    LetDecl {
        pat: Pattern,
        type_constraint: Option<AnyTypeExpr>,
        value: Expr,
    },
    Block {
        stmts: Vec<Stmt>,
    },
    Expr(Expr),
}
