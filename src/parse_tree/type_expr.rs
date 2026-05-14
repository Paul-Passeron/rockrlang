use crate::{common::symbols::Symbol, parse_tree::Spanned};

pub type TypeExpr = Spanned<TypeExprDesc>;

#[derive(PartialEq, Eq, Hash, Debug)]
pub enum TypeExprDesc {
    Named {
        name: Symbol,
        args: Vec<AnyTypeExpr>,
    },
    NameResolved {
        from: Symbol,
        to: Box<TypeExprDesc>,
    },
    Pointer(Box<TypeExpr>),
    Slice {
        ty: Box<TypeExpr>,
        len: Option<usize>,
    },
}

pub type AnyTypeExpr = Spanned<AnyTypeExprDesc>;

#[derive(PartialEq, Eq, Hash, Debug)]
pub enum AnyTypeExprDesc {
    Any,
    Known(TypeExprDesc),
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ConstrainedType {
    ty: TypeExpr,
    constraints: Vec<TypeExpr>,
}
