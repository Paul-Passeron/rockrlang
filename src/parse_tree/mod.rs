#![allow(dead_code)]

use std::{fmt, hash::Hash};

use crate::common::location::Span;

pub mod expr;
pub mod pattern;
pub mod stmt;
pub mod top_level;
pub mod type_expr;

pub struct Spanned<T> {
    pub data: T,
    pub span: Span,
}

impl<T: PartialEq> PartialEq for Spanned<T> {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}
impl<T> Eq for Spanned<T> where Spanned<T>: PartialEq {}

impl<T: Hash> Hash for Spanned<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.data.hash(state);
        self.span.hash(state);
    }
}

impl<T> Spanned<T> {
    pub fn new(data: T, span: Span) -> Self {
        Self { data, span }
    }

    pub fn as_ref(&self) -> Spanned<&T> {
        Spanned::new(&self.data, self.span.clone())
    }
}

impl<T: fmt::Debug> fmt::Debug for Spanned<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.data.fmt(f)
    }
}
