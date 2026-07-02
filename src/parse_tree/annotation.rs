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

use crate::{common::symbols::Symbol, parse_tree::type_expr::AstTypeExpr};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AstAnnotation {
    pub items: Vec<AstAnnotationItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AstAnnotationItem {
    Flag(Symbol),

    Call { name: Symbol, args: Vec<AstAnnotationArg> },
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
