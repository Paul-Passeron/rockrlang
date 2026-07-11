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
    check::{mir::check_mir, thir::validate_thir},
    hir::function_ast,
    name_resolve::type_expr::get_templates_of_fun,
    ril::{FunctionId, TypeRef},
    thir::thir_body,
    thir_to_mir::{_mir, MIRKey, mir},
    typecheck::type_check_function,
};
use std::collections::HashSet;

pub fn check_fundef(db: &dyn Db, fdef: FunctionId) {
    if let Some(thir) = thir_body(db, fdef) {
        // println!("{}", thir.display(db));
        validate_thir(db, thir.as_ref());
    }

    for (fdef, subs) in reachable_mir_instances(db, fdef) {
        process_mir_instance(db, fdef, subs.clone());
        if fdef.has_body(db) {
            let the_mir = mir(db, fdef, subs);
            check_mir(db, the_mir);
        }
    }
}

pub(crate) fn reachable_mir_instances(
    db: &dyn Db,
    root: FunctionId,
) -> Vec<(FunctionId, Vec<TypeRef>)> {
    let mut seen: HashSet<MIRKey> = HashSet::new();
    let mut worklist: Vec<(FunctionId, Vec<TypeRef>)> = vec![];
    let mut res = vec![];

    if !get_templates_of_fun(db, root.interned()).is_empty() {
        // Gneric so not a monomorphization root
        return res;
    }
    worklist.push((root, vec![]));

    while let Some((fdef, subs)) = worklist.pop() {
        // Invariant: `subs` is fully concrete — no Params survive here.
        debug_assert!(
            subs.iter().all(|ty| matches!(ty, TypeRef::Concrete(_))),
            "instance ({}, {:?}) reached the worklist with unresolved params",
            fdef.name(db).display(db),
            subs.iter().map(|t| t.to_string(db)).collect::<Vec<_>>(),
        );

        if !seen.insert(MIRKey::new(db, fdef, subs.clone())) {
            continue;
        }
        res.push((fdef, subs.clone()));

        if !function_ast(db, fdef.interned()).inner(db).has_body() {
            continue; // extern: no calls to walk
        }

        let Some(tc) = type_check_function(db, fdef) else {
            continue;
        };
        for call_info in tc.call_infos(db).values() {
            // Compose: the callee's substitution is written in terms of
            // the *caller's* params; instantiate it with our own subs.
            let callee_subs: Vec<TypeRef> = call_info
                .substitution
                .iter()
                .map(|ty| ty.with_substitution(db, &subs))
                .collect();

            worklist.push((call_info.callee, callee_subs));
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
    if !fdef.has_body(db) {
        return;
    }

    let the_mir = _mir(db, key);

    check_mir(db, the_mir);

    // let subs = key.subs(db);
    // println!(
    //     "{}: {}{}",
    //     fdef.span(db).start().loc_info(db),
    //     fdef.called_to_string(db),
    //     if subs.is_empty() {
    //         String::new()
    //     } else {
    //         format!(
    //             " with substitutions <{}>",
    //             subs.iter().map(|ty| ty.to_string(db)).join(", ")
    //         )
    //     }
    // );

    // println!("{}", the_mir.display(db),);

    // let liveness = the_mir.as_ref().liveness(db);
    // println!("Liveness analysis:");
    // println!("{liveness}");

    // let init = the_mir.init_tracking(db);
    // println!("init analysis:");
    // println!("{init}");

    // let loans = the_mir.loans(db);
    // println!("loans analysis:");
    // println!("{}", loans.display(db));
}
