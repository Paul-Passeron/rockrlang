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

use crate::{Db, check::thir::validate_thir, name_resolve::type_expr::get_templates_of_fun, ril::FunctionId, thir::thir_body, thir_to_mir::mir};

pub fn check_fundef(db: &dyn Db, fdef: FunctionId) {
    if let Some(thir) = thir_body(db, fdef) {
        println!("{}", thir.display(db));
        validate_thir(db, thir.as_ref());

        let templates = get_templates_of_fun(db, fdef.interned());
        if templates.is_empty() {
            let mir = mir(db, fdef, vec![]);
            println!("{}", mir.display(db))
        }
        // TODO: transitively compute other mirs
    }
}
