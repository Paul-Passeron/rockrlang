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
    check::thir::{return_check::check_return, sanity_check::sanity_check},
    compiler::diagnostic::{Diag, Severity},
    ril::{FunctionId, InternedFunctionId},
    thir::{Thir, thir_body},
};

pub mod return_check;
pub mod sanity_check;

pub fn validate_thir(db: &dyn Db, thir: &Thir) {
    check_return(db, thir);
    sanity_check(db, thir);
}

#[salsa::tracked]
pub fn checked_thir_body<'db>(db: &'db dyn Db, function: InternedFunctionId<'db>) {
    if let Some(thir) = thir_body(db, function.into()) {
        validate_thir(db, thir);
    }
}

pub fn thir_is_valid(db: &dyn Db, function: FunctionId) -> bool {
    checked_thir_body::accumulated::<Diag>(db, function.interned())
        .iter()
        .all(|diag| diag.severity != Severity::Error)
}
