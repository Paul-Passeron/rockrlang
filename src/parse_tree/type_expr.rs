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

use crate::{common::symbols::Symbol, parse_tree::Spanned, parser::ParseError};

pub type AstTypeExpr = Spanned<AstTypeExprDesc>;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum AstTypeExprDesc {
    Named { name: Spanned<Symbol>, args: Vec<AstAnyTypeExpr> },
    NameResolved { from: Spanned<Symbol>, to: Box<AstTypeExpr> },
    Ref { mutable: bool, pointee: Box<AstAnyTypeExpr> },
    Pointer { mutable: bool, pointee: Box<AstAnyTypeExpr> },
    Slice { ty: Box<AstAnyTypeExpr>, len: Option<usize> },
    Tuple(Vec<AstAnyTypeExpr>),
    Error(ParseError),
}

pub type AstAnyTypeExpr = Spanned<AstAnyTypeExprDesc>;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum AstAnyTypeExprDesc {
    Any,
    Known(AstTypeExprDesc),
}

#[allow(dead_code)]
impl AstAnyTypeExpr {
    pub fn as_known(&self) -> Option<AstTypeExpr> {
        if let AstAnyTypeExprDesc::Known(ty) = &self.data {
            Some(AstTypeExpr::new(ty.clone(), self.span))
        } else {
            None
        }
    }
}

impl From<AstTypeExpr> for AstAnyTypeExpr {
    fn from(ty: AstTypeExpr) -> Self {
        AstAnyTypeExpr {
            data: AstAnyTypeExprDesc::Known(ty.data),
            span: ty.span,
        }
    }
}
