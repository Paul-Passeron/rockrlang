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

use crate::typecheck::inference::InferenceCtx;

use super::InferTy;
use ena::unify::UnifyKey;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InferVar(pub usize);

impl fmt::Display for InferVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}", self.0)
    }
}

impl From<InferVar> for InferTy {
    fn from(value: InferVar) -> Self {
        Self::Var(value)
    }
}

impl From<&InferVar> for InferTy {
    fn from(value: &InferVar) -> Self {
        Self::Var(*value)
    }
}

impl UnifyKey for InferVar {
    type Value = Option<InferTy>;

    fn index(&self) -> u32 {
        self.0 as u32
    }

    fn from_index(u: u32) -> Self {
        Self(u as usize)
    }

    fn tag() -> &'static str {
        "InferVar"
    }
}

impl InferenceCtx<'_> {
    pub fn fresh_var(&mut self) -> InferVar {
        self.table.new_key(None)
    }
}
