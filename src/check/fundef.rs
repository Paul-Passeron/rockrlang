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

use salsa::Accumulator;

use crate::{Db, check::Diag, ril::FunctionId, thir::thir_body};

pub fn check_fundef<'db>(db: &'db dyn Db, fdef: FunctionId) {
    let span = fdef.span(db);
    let thir = thir_body(db, fdef);
    if let Some(_) = thir {
        Diag::todo("Check the thir output".into(), span).accumulate(db);
    } else {
        Diag::todo("Check something else I guess".into(), span).accumulate(db);
    }
}
