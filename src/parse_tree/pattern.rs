use crate::{common::symbols::Symbol, parse_tree::Spanned};

pub type Pattern = Spanned<PatternDesc>;

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum NamedPattern {
    Constructor { name: Symbol, args: Vec<Pattern> },
    NameResolved { from: Symbol, to: Box<NamedPattern> },
    Tuple { fields: Vec<Pattern> },
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum PatternDesc {
    Named(NamedPattern),
    Any,
}
