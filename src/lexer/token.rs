#![allow(dead_code)]
#![allow(unused_variables)]

use std::fmt;

use crate::common::{
    location::Span,
    symbols::{StrLit, Symbol},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Identifier(Symbol),
    IntLit(i64),
    CharLit(char),
    StrLit(StrLit),
    CStrLit(StrLit),
    Directive(Symbol),
    Hashed(Symbol), // #symbol

    // Keywords
    Let,
    Mut,
    Fun,
    Match,
    If,
    Else,
    While,
    Return,
    Struct,
    Enum,
    Defer,
    Impl,
    Module,
    Interface,
    For,
    In,
    Type,
    Break,
    Use,
    Static,
    True,
    False,
    Meta,
    Eq,

    // Operators
    Plus,
    Minus,
    Mult,
    Div,
    Modulo,
    Geq,
    Gt,
    Leq,
    Lt,
    Access,
    EqEq,
    Diff,
    And,
    BitAnd,
    Or,
    BitOr,
    BitXor,
    Not,
    Deref,
    AddressOf,
    PlusEq,
    MinusEq,
    MultEq,
    DivEq,
    ModuloEq,

    // Delimeters
    Colon,
    Semicolon,
    Comma,
    BigArrow,
    SmallArrow,
    OpenPar,
    ClosePar,
    OpenSqr,
    CloseSqr,
    OpenBra,
    CloseBra,
    DotDotDot,
    DotDot,
    Dot,
    HashPound,
}

pub struct TokenKindDisplay<'db, 'a> {
    pub kind: &'a TokenKind,
    pub db: &'db dyn crate::Db,
}

impl TokenKind {
    pub fn display<'a, 'db>(&'a self, db: &'db dyn crate::Db) -> TokenKindDisplay<'db, 'a> {
        TokenKindDisplay { kind: self, db }
    }
}

impl<'db> fmt::Display for TokenKindDisplay<'db, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            TokenKind::Identifier(name) => write!(f, "{}", name.interned().contents(self.db)),
            TokenKind::IntLit(value) => write!(f, "{}", value),
            TokenKind::CharLit(c) => write!(f, "{}", c),
            TokenKind::StrLit(s) => write!(f, "\"{}\"", s.interned().contents(self.db)),
            TokenKind::CStrLit(s) => write!(f, "c\"{}\"", s.interned().contents(self.db)),
            TokenKind::Directive(d) => write!(f, "@{}", d.interned().contents(self.db)),
            TokenKind::Hashed(symbol) => write!(f, "#{}", symbol.interned().contents(self.db)),
            TokenKind::HashPound => write!(f, "#"),
            TokenKind::Let => write!(f, "let"),
            TokenKind::Mut => write!(f, "mut"),
            TokenKind::Fun => write!(f, "fun"),
            TokenKind::If => write!(f, "if"),
            TokenKind::Match => write!(f, "match"),
            TokenKind::Else => write!(f, "else"),
            TokenKind::While => write!(f, "while"),
            TokenKind::Return => write!(f, "return"),
            TokenKind::Struct => write!(f, "struct"),
            TokenKind::Enum => write!(f, "enum"),
            TokenKind::Defer => write!(f, "defer"),
            TokenKind::Impl => write!(f, "impl"),
            TokenKind::Module => write!(f, "module"),
            TokenKind::Interface => write!(f, "interface"),
            TokenKind::Plus => write!(f, "+"),
            TokenKind::Minus => write!(f, "-"),
            TokenKind::Mult => write!(f, "*"),
            TokenKind::Div => write!(f, "/"),
            TokenKind::Modulo => write!(f, "%"),
            TokenKind::Geq => write!(f, ">="),
            TokenKind::Gt => write!(f, ">"),
            TokenKind::Leq => write!(f, "<="),
            TokenKind::Lt => write!(f, "<"),
            TokenKind::Access => write!(f, "::"),
            TokenKind::Eq => write!(f, "="),
            TokenKind::EqEq => write!(f, "=="),
            TokenKind::Diff => write!(f, "!="),
            TokenKind::And => write!(f, "&&"),
            TokenKind::BitAnd => write!(f, "&"),
            TokenKind::Or => write!(f, "||"),
            TokenKind::BitOr => write!(f, "|"),
            TokenKind::BitXor => write!(f, "^"),
            TokenKind::Not => write!(f, "!"),
            TokenKind::AddressOf => write!(f, "@"),
            TokenKind::Deref => write!(f, "$"),
            TokenKind::PlusEq => write!(f, "+="),
            TokenKind::MinusEq => write!(f, "-="),
            TokenKind::MultEq => write!(f, "*="),
            TokenKind::DivEq => write!(f, "/="),
            TokenKind::ModuloEq => write!(f, "%="),
            TokenKind::Colon => write!(f, ":"),
            TokenKind::Comma => write!(f, ","),
            TokenKind::Semicolon => write!(f, ";"),
            TokenKind::BigArrow => write!(f, "=>"),
            TokenKind::SmallArrow => write!(f, "->"),
            TokenKind::OpenPar => write!(f, "("),
            TokenKind::ClosePar => write!(f, ")"),
            TokenKind::OpenSqr => write!(f, "["),
            TokenKind::CloseSqr => write!(f, "]"),
            TokenKind::OpenBra => write!(f, "{{"),
            TokenKind::CloseBra => write!(f, "}}"),
            TokenKind::Dot => write!(f, "."),
            TokenKind::DotDot => write!(f, ".."),
            TokenKind::DotDotDot => write!(f, "..."),
            TokenKind::For => write!(f, "for"),
            TokenKind::In => write!(f, "in"),
            TokenKind::Type => write!(f, "type"),
            TokenKind::Break => write!(f, "break"),
            TokenKind::Use => write!(f, "use"),
            TokenKind::Static => write!(f, "static"),
            TokenKind::True => write!(f, "true"),
            TokenKind::False => write!(f, "false"),
            TokenKind::Meta => write!(f, "meta"),
        }
    }
}

#[derive(Clone)]
pub struct Token {
    pub location: Span,
    pub kind: TokenKind,
}
