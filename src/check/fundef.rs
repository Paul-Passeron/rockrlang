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
    Db,
    check::{Diag, Diagnostics},
    ril::FunctionId,
};

pub fn check_fundef<'db>(db: &'db dyn Db, fdef: FunctionId) -> Diagnostics<'db> {
    let span = fdef.name_span(db);
    Diagnostics::new(
        db,
        vec![Diag::todo(db, "implement check_fundef".into(), span)],
    )
}
