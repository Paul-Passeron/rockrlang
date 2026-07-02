/* Rockr programming language
Copyright (C) 2026  NoRezap

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

use std::{fmt, hash::Hash};

use crate::{common::location::Span, parse_tree::annotation::AstAnnotation};

pub mod annotation;
pub mod expr;
pub mod pattern;
pub mod stmt;
pub mod top_level;
pub mod type_expr;

pub struct Spanned<T> {
    pub data: T,
    pub annotations: Vec<AstAnnotation>,
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
    pub fn new(data: T, annotations: Vec<AstAnnotation>, span: Span) -> Self {
        Self { data, annotations, span }
    }
}

impl<T: fmt::Debug> fmt::Debug for Spanned<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.annotations.is_empty() {
            self.data.fmt(f)
        } else {
            f.debug_struct("Annotated")
                .field("data", &self.data)
                .field("annotations", &self.annotations)
                .finish()
        }
    }
}

impl<T: Clone> Clone for Spanned<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            annotations: self.annotations.clone(),
            span: self.span,
        }
    }
}
