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

use crate::{
    hir::signature::{FunctionSignature, get_sig_of_function},
    model::{FunctionId, type_printer::TypePrinter},
};
use itertools::Itertools;
use rockr_base::Db;
use std::fmt::Write;

impl TypePrinter {
    pub fn function_sig_to_string(&self, db: &dyn Db, sig: &FunctionSignature) -> String {
        let mut s = String::new();
        let f = &mut s;
        let mut aux = || -> std::fmt::Result {
            write!(f, "{}", sig.name.display(db))?;
            if !sig.added_templates.is_empty() || !sig.implicit_templates.is_empty() {
                write!(f, "<")?;
                for (i, t) in
                    sig.implicit_templates.iter().chain(&sig.added_templates).enumerate()
                {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "T{i}")?;
                    if !t.is_empty() {
                        write!(
                            f,
                            ": {}",
                            t.iter()
                                .map(|c| self.interface_ref_to_string(db, *c))
                                .collect_vec()
                                .join(" + ")
                        )?;
                    }
                }
                write!(f, ">")?;
            }
            write!(f, "(")?;
            if let Some(arg) = sig.zelf {
                write!(f, "{arg}")?;
                if !sig.args.is_empty() {
                    write!(f, ", ")?;
                }
            }
            write!(
                f,
                "{}): {}",
                sig.args
                    .iter()
                    .map(|arg| format!(
                        "{}: {}",
                        arg.0.display(db),
                        self.type_ref_to_string(db, arg.1)
                    ))
                    .collect_vec()
                    .join(", "),
                self.type_ref_to_string(db, sig.ret)
            )?;
            Ok(())
        };
        aux().unwrap();
        s
    }

    pub fn function_id_to_string(&self, db: &dyn Db, function_id: FunctionId) -> String {
        let sig = get_sig_of_function(db, function_id.interned());
        Self::join_path(
            &self.scope_owner_to_string(db, function_id.parent(db)),
            &self.function_sig_to_string(db, sig),
        )
    }
}
