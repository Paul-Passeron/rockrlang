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
    common::timing::{Counts, Phase, timed},
    hir::{HirBody, hir_body},
    resolved::{FunctionId, InternedFunctionId},
    thir::Thir,
    typecheck::{TypeCheckResults, type_check_function},
};
pub use builder::ThirBuilder;
use translator::ThirTranslator;

mod builder;
mod refs;
mod translator;

pub fn thir_body_from_hir<'db>(
    db: &'db dyn Db,
    hir: HirBody<'db>,
    tc: TypeCheckResults<'db>,
) -> Thir {
    ThirTranslator::new(db, hir, tc).translate()
}

pub fn thir_body(db: &dyn Db, function: FunctionId) -> Option<&Thir> {
    thir_body_impl(db, function.interned()).as_ref()
}

#[salsa::tracked]
fn thir_body_impl<'db>(
    db: &'db dyn Db,
    function: InternedFunctionId<'db>,
) -> Option<Thir> {
    timed(
        db,
        Phase::Thir,
        || {
            let f_id: FunctionId = function.into();
            let hir = hir_body(db, function.into())?;
            let tc = type_check_function(db, f_id)?;
            let thir = thir_body_from_hir(db, hir, tc);
            Some(thir)
        },
        |res| Counts {
            functions: usize::from(res.is_some()),
            stmts: res.as_ref().map(|t| t.root.len()),
            exprs: res.as_ref().map(|t| t.exprs.len()),
        },
    )
}
