use crate::{common::symbols::Symbol, parse_tree::type_expr::AstTypeExpr};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AstAnnotation {
    pub items: Vec<AstAnnotationItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AstAnnotationItem {
    Flag(Symbol),

    Call {
        name: Symbol,
        args: Vec<AstAnnotationArg>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AstAnnotationArg {
    // Symbol(Symbol),
    Type(AstTypeExpr),
}

#[allow(dead_code)]
impl AstAnnotationItem {
    pub fn name(&self) -> Symbol {
        match self {
            AstAnnotationItem::Flag(name) => *name,
            AstAnnotationItem::Call { name, .. } => *name,
        }
    }

    pub fn args(&self) -> &[AstAnnotationArg] {
        match self {
            AstAnnotationItem::Flag(_) => &[],
            AstAnnotationItem::Call { args, .. } => args,
        }
    }
}
