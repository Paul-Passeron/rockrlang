use crate::{common::symbols::Symbol, parse_tree::Spanned};

pub type AstTypeExpr = Spanned<AstTypeExprDesc>;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum AstTypeExprDesc {
    Named {
        name: Symbol,
        args: Vec<AstAnyTypeExpr>,
    },
    NameResolved {
        from: Symbol,
        to: Box<AstTypeExpr>,
    },
    Pointer(Box<AstTypeExpr>),
    Slice {
        ty: Box<AstTypeExpr>,
        len: Option<usize>,
    },
    Tuple(Vec<AstTypeExpr>),
}

pub type AstAnyTypeExpr = Spanned<AstAnyTypeExprDesc>;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum AstAnyTypeExprDesc {
    Any,
    Known(AstTypeExprDesc),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AstConstrainedType {
    ty: AstTypeExpr,
    constraints: Vec<AstTypeExpr>,
}
