use crate::common::symbols::Symbol;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AstAnnotation {
    pub items: Vec<AstAnnotationItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AstAnnotationItem {
    Named(Symbol),
}
