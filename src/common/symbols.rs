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

use std::marker::PhantomData;

use crate::Db;

#[salsa::interned]
pub struct InternedSymbol {
    pub contents: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Symbol(salsa::Id);

impl<'db> From<InternedSymbol<'db>> for Symbol {
    fn from(s: InternedSymbol<'db>) -> Self {
        Symbol(s.0)
    }
}

impl Symbol {
    pub fn new(db: &dyn Db, s: impl ToString) -> Self {
        Self::from(InternedSymbol::new(db, s.to_string()))
    }

    pub fn interned<'db>(&self) -> InternedSymbol<'db> {
        InternedSymbol(self.0, PhantomData)
    }

    pub fn display(&self, db: &dyn Db) -> String {
        self.interned().contents(db).to_string()
    }

    pub fn to_string(&self, db: &dyn Db) -> String {
        self.display(db).to_string()
    }
}

#[salsa::interned]
pub struct InternedStrLit {
    pub contents: String,
}

/// A 'static-compatible StrLit ID, safe to store anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StrLit(salsa::Id);

impl<'db> From<InternedStrLit<'db>> for StrLit {
    fn from(s: InternedStrLit<'db>) -> Self {
        StrLit(s.0)
    }
}

impl StrLit {
    pub fn new(db: &dyn Db, s: impl ToString) -> Self {
        Self::from(InternedStrLit::new(db, s.to_string()))
    }

    pub fn interned(&self) -> InternedStrLit<'_> {
        InternedStrLit(self.0, PhantomData)
    }

    pub fn display(&self, db: &dyn Db) -> String {
        self.interned().contents(db).to_string()
    }
}
