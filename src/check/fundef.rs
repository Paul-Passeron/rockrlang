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

use std::collections::HashSet;

use itertools::Itertools;

use crate::{
    Db,
    check::{mir::check_mir, thir::validate_thir},
    hir::function_ast,
    mir::{
        analysis::{
            MIRAnalysis, init_tracking::MIRInitAnalysis, liveness::MIRLivenessAnalysis,
        },
        passes::{MIRPass, dead_code_elimination::DeadCodeElimination},
    },
    name_resolve::type_expr::get_templates_of_fun,
    ril::{FunctionId, TypeRef},
    thir::thir_body,
    thir_to_mir::{_mir, MIRKey},
    typecheck::type_check_function,
};

pub fn check_fundef(db: &dyn Db, fdef: FunctionId) {
    if let Some(thir) = thir_body(db, fdef) {
        validate_thir(db, thir.as_ref());
    }

    for (fdef, subs) in reachable_mir_instances(db, fdef) {
        process_mir_instance(db, fdef, subs);
    }
}

fn reachable_mir_instances(
    db: &dyn Db,
    root: FunctionId,
) -> Vec<(FunctionId, Vec<TypeRef>)> {
    let mut seen: HashSet<MIRKey> = HashSet::new();
    let mut worklist: Vec<(FunctionId, Vec<TypeRef>)> = vec![];
    let mut res = vec![];

    if !function_ast(db, root.into()).inner(db).has_body() {
        return res;
    }

    if get_templates_of_fun(db, root.interned()).is_empty() {
        worklist.push((root, vec![]));
    }

    while let Some((fdef, subs)) = worklist.pop() {
        if !seen.insert(MIRKey::new(db, fdef, subs.clone())) {
            continue;
        }
        res.push((fdef, subs.clone()));

        let Some(tc) = type_check_function(db, fdef) else {
            // ensure there is a body to typecheck
            continue;
        };
        for call_info in tc.call_infos(db).values() {
            let callee_templates = get_templates_of_fun(db, call_info.callee.interned());
            if !callee_templates.is_empty() {
                worklist.push((call_info.callee, call_info.substitution.clone()));
            } else if !seen.contains(&MIRKey::new(db, call_info.callee, vec![])) {
                worklist.push((call_info.callee, vec![]));
            }
        }
    }

    res
}

fn process_mir_instance(db: &dyn Db, fdef: FunctionId, subs: Vec<TypeRef>) {
    _process_mir_instance(db, MIRKey::new(db, fdef, subs));
}

#[salsa::tracked]
fn _process_mir_instance<'db>(db: &'db dyn Db, key: MIRKey<'db>) {
    let fdef = key.fdef(db);
    let subs = key.subs(db);

    let the_mir = _mir(db, key);

    check_mir(db, the_mir.as_ref());
    
    println!(
        "{}: {}{}",
        fdef.span(db).start().loc_info(db),
        fdef.called_to_string(db),
        if subs.is_empty() {
            String::new()
        } else {
            format!(
                " with substitutions <{}>",
                subs.iter().map(|ty| ty.to_string(db)).join(", ")
            )
        }
    );

    println!("{}", the_mir.display(db),);

    let liveness = the_mir.as_ref().liveness(db);
    println!("Liveness analysis:");
    println!("{liveness}");

    let init = the_mir.init_tracking(db);
    println!("init analysis:");
    println!("{init}");

    let loans = the_mir.loans(db);
    println!("loans analysis:");
    println!("{}", loans.display(db));

    // let dce_mir = DeadCodeElimination.run(db, the_mir.as_ref());

    // println!("AFTER DCE {}", dce_mir.display(db));
}
