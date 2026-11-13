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

use crate::Db;
use std::{marker::PhantomData, ops::Deref};

#[salsa::interned]
pub struct InternedSymbol {
    pub contents: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Symbol(salsa::Id);

impl<'db> From<InternedSymbol<'db>> for Symbol {
    fn from(s: InternedSymbol<'db>) -> Self {
        Self(s.0)
    }
}

impl Symbol {
    pub fn new(db: &dyn Db, s: impl Deref<Target = str>) -> Self {
        Self::from(InternedSymbol::new(db, s.to_string()))
    }

    pub fn interned<'db>(self) -> InternedSymbol<'db> {
        InternedSymbol(self.0, PhantomData)
    }

    pub fn display(self, db: &dyn Db) -> String {
        self.interned().contents(db).clone()
    }

    pub fn to_string(self, db: &dyn Db) -> String {
        self.display(db)
    }
    pub fn is(self, db: &dyn Db, s: &str) -> bool {
        self.interned().contents(db) == s
    }
}
