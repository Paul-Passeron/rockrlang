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

use crate::{common::symbols::Symbol, parse_tree::Spanned};

pub type AstPattern = Spanned<AstPatternDesc>;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum StructFieldPattern {
    Rebind { name: Symbol, pattern: AstPattern },
    Name(Symbol),
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AstConstructFields {
    TupleFields(Vec<AstPattern>),
    StructFields(Vec<StructFieldPattern>),
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AstNamedPattern {
    Mut {
        name: Symbol,
    },
    Constructor {
        name: Symbol,
        args: AstConstructFields,
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
    IntLiteral(i64),
    Any,
}
