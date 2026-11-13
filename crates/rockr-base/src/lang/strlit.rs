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
pub struct InternedStrLit {
    pub contents: String,
}

/// A 'static-compatible `StrLit` ID, safe to store anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StrLit(salsa::Id);

impl<'db> From<InternedStrLit<'db>> for StrLit {
    fn from(s: InternedStrLit<'db>) -> Self {
        Self(s.0)
    }
}

impl StrLit {
    pub fn new(db: &dyn Db, s: impl Deref<Target = str>) -> Self {
        Self::from(InternedStrLit::new(db, s.deref().to_owned()))
    }

    pub fn interned<'db>(self) -> InternedStrLit<'db> {
        InternedStrLit(self.0, PhantomData)
    }

    pub fn display(self, db: &dyn Db) -> String {
        self.interned().contents(db).clone()
    }
}
