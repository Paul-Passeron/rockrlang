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

use super::{InferTy, TypeDefId};
use crate::{
    name_resolve::definition::get_module_pretty_name,
    ril::{BuiltinTypeId, display::Display},
};
use std::fmt;

impl InferTy {
    pub fn display<'a>(&'a self, db: &'a dyn crate::Db) -> Display<'a, &'a Self> {
        Display::new(db, self)
    }
}

impl fmt::Display for Display<'_, &InferTy> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            InferTy::Var(infer_var) => {
                write!(f, "{}", infer_var)
            }
            InferTy::Adt { def, fields } => match def {
                TypeDefId::Builtin(id) => {
                    let id = *id;
                    if id == BuiltinTypeId::int(self.db) {
                        write!(f, "int")
                    } else if id == BuiltinTypeId::bool(self.db) {
                        write!(f, "bool")
                    } else if id == BuiltinTypeId::char(self.db) {
                        write!(f, "char")
                    } else if id == BuiltinTypeId::void(self.db) {
                        write!(f, "void")
                    } else if id == BuiltinTypeId::never(self.db) {
                        write!(f, "never")
                    } else if id == BuiltinTypeId::ptr(self.db) {
                        write!(f, "*const {}", fields[0].display(self.db))
                    } else if id == BuiltinTypeId::mut_ptr(self.db) {
                        write!(f, "*mut {}", fields[0].display(self.db))
                    } else if id == BuiltinTypeId::ref_(self.db) {
                        write!(f, "&{}", fields[0].display(self.db))
                    } else if id == BuiltinTypeId::mut_ref(self.db) {
                        write!(f, "&mut {}", fields[0].display(self.db))
                    } else if id == BuiltinTypeId::slice(self.db) {
                        write!(f, "[{}]", fields[0].display(self.db))
                    } else if id == BuiltinTypeId::tuple(self.db) {
                        write!(f, "(")?;
                        for (i, field) in fields.iter().enumerate() {
                            if i > 0 {
                                write!(f, ", ")?;
                            }
                            write!(f, "{}", field.display(self.db))?;
                        }
                        write!(f, ")")
                    } else {
                        write!(f, "{{unknown builtin}}")
                    }
                }
                def => {
                    write!(
                        f,
                        "{}::{}",
                        get_module_pretty_name(self.db, def.parent(self.db).interned()),
                        def.name(self.db).display(self.db)
                    )?;

                    if !fields.is_empty() {
                        write!(f, "<")?;
                        for (i, field) in fields.iter().enumerate() {
                            if i > 0 {
                                write!(f, ", ")?;
                            }
                            write!(f, "{}", field.display(self.db))?;
                        }
                        write!(f, ">")?;
                    }

                    Ok(())
                }
            },
            InferTy::Param(type_param_id) => write!(f, "T{}", type_param_id.0),
        }
    }
}
