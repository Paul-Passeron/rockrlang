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
    check::thir::validate_thir,
    mir::passes::{MIRPass, dead_code_elimination::DeadCodeElimination},
    name_resolve::type_expr::get_templates_of_fun,
    ril::FunctionId,
    thir::thir_body,
    thir_to_mir::mir,
    typecheck::type_check_function,
};

pub fn check_fundef(db: &dyn Db, fdef: FunctionId) {
    if let Some(thir) = thir_body(db, fdef) {
        validate_thir(db, thir.as_ref());

        let templates = get_templates_of_fun(db, fdef.interned());
        if templates.is_empty() {
            let the_mir = mir(db, fdef, vec![]);
            let dce_mir = DeadCodeElimination.run(db, the_mir.as_ref());
            println!(
                "{}: {}\nBEFORE DCE {}\nAFTER DCE {}",
                fdef.span(db).start().loc_info(db),
                fdef.called_to_string(db),
                the_mir.display(db),
                dce_mir.display(db),
            );
            if let Some(tc) = type_check_function(db, fdef) {
                for call_info in tc.call_infos(db).values() {
                    let callee_templates =
                        get_templates_of_fun(db, call_info.callee.interned());
                    if !callee_templates.is_empty() {
                        let other_mir =
                            mir(db, call_info.callee, call_info.substitution.clone());
                        let dce_mir = DeadCodeElimination.run(db, other_mir.as_ref());

                        println!(
                            "{}: {}\nBEFORE DCE {}\nAFTER DCE {}",
                            call_info.callee.span(db).start().loc_info(db),
                            call_info.callee.called_to_string(db),
                            other_mir.display(db),
                            dce_mir.display(db),
                        );
                    }
                }
            }
        }
    }
}
