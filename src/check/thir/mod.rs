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

use crate::{
    Db, check::thir::return_check::check_return, compiler::diagnostic::Diag,
    thir::Thir,
};

pub mod return_check;

pub fn validate_thir(db: &dyn Db, thir: &Thir) {
    check_return(db, thir);
    let span = thir.id.span(db);
    Diag::todo(
        format!("Implement validate_thir ({}:{})", file!(), line!()),
        span,
    )
    .accumulate(db);
}
