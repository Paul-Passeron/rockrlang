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

use std::fmt::{self};

use crate::{
    Db,
    ril::{
        TypeDefId,
        display::{Display, RilDisplay},
    },
    thir::inference::{InferTy, InferenceCtx},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CanonTy {
    Hole,
    Adt { id: TypeDefId, args: Box<[CanonTy]> },
    Param(usize),
}

impl<'db> InferenceCtx<'db> {
    pub fn canonize(&mut self, ty: &InferTy) -> CanonTy {
        match self.find(ty) {
            InferTy::Var(_) => CanonTy::Hole,
            InferTy::Adt { def, fields } => CanonTy::Adt {
                id: def,
                args: fields.iter().map(|f| self.canonize(f)).collect(),
            },
            InferTy::Param(type_param_id) => CanonTy::Param(type_param_id.0),
        }
    }
}

impl CanonTy {
    pub fn display(&self, db: &dyn Db) -> impl fmt::Display {
        Display { value: self, db }
    }
}

impl fmt::Display for Display<'_, &CanonTy> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            CanonTy::Hole => write!(f, "_"),
            CanonTy::Adt { id, args } => {
                write!(f, "{}", id.display(self.db))?;
                if !args.is_empty() {
                    write!(
                        f,
                        "({})",
                        args.iter()
                            .map(|a| a.display(self.db).to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )?;
                }
                Ok(())
            }
            CanonTy::Param(n) => write!(f, "T{}", n),
        }
    }
}
