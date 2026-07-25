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

use crate::{Db, hir::HirBody, thir::Thir, typecheck::TypeCheckResults};

mod builder;
mod refs;
mod translator;

pub use builder::ThirBuilder;
use translator::ThirTranslator;

pub fn thir_body_from_hir<'db>(
    db: &'db dyn Db,
    hir: HirBody<'db>,
    tc: TypeCheckResults<'db>,
) -> Thir {
    ThirTranslator::new(db, hir, tc).translate()
}
