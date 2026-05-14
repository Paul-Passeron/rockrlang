use crate::common::symbols::Symbol;

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Annotation {
    pub items: Vec<AnnotationItem>,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum AnnotationItem {
    Named(Symbol),
}
