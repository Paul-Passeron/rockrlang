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
    Db, check::thir::validate_thir, name_resolve::type_expr::get_templates_of_fun,
    ril::FunctionId, thir::thir_body, thir_to_mir::mir, typecheck::type_check_function,
};

pub fn check_fundef(db: &dyn Db, fdef: FunctionId) {
    if let Some(thir) = thir_body(db, fdef) {
        println!("{}", thir.display(db));
        validate_thir(db, thir.as_ref());

        let templates = get_templates_of_fun(db, fdef.interned());
        if templates.is_empty() {
            let the_mir = mir(db, fdef, vec![]);
            println!("{}:", fdef.sig_to_string(db));
            println!("{}", the_mir.display(db));
            if let Some(tc) = type_check_function(db, fdef) {
                for call_info in tc.call_infos(db).values() {
                    let callee_templates =
                        get_templates_of_fun(db, call_info.callee.interned());
                    if !callee_templates.is_empty() {
                        println!("{}:", call_info.callee.sig_to_string(db));
                        println!("{}", the_mir.display(db));
                        mir(db, call_info.callee, call_info.substitution.clone());
                    }
                }
            }
        }
    }
}
