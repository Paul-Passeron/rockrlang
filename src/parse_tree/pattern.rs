use crate::{common::symbols::Symbol, parse_tree::Spanned};

pub type AstPattern = Spanned<AstPatternDesc>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AstNamedPattern {
    Mut {
        name: Symbol,
    },
    Constructor {
        name: Symbol,
        args: Vec<AstPattern>,
    },
    NameResolved {
        from: Symbol,
        to: Box<AstNamedPattern>,
    },
    Tuple {
        fields: Vec<AstPattern>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AstPatternDesc {
    Named(AstNamedPattern),
    Any,
}
