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
    Ref {
        mutable: bool,
        pointee: Box<AstTypeExpr>,
    },
    Pointer {
        mutable: bool,
        pointee: Box<AstTypeExpr>,
    },
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

impl From<AstTypeExpr> for AstAnyTypeExpr {
    fn from(ty: AstTypeExpr) -> Self {
        AstAnyTypeExpr {
            data: AstAnyTypeExprDesc::Known(ty.data),
            annotations: ty.annotations,
            span: ty.span,
        }
    }
}
