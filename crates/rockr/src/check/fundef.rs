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
    check::{
        mir::check_mir,
        thir::{checked_thir_body, thir_is_valid},
    },
    common::timing::{Counts, Phase, timed},
    mir::{MIRKey, passes::dead_code_elimination::dce},
    name_resolve::type_expr::get_templates_of_fun,
    resolved::{
        FunctionId,
        concrete_ty::{Substitution, concretize_fid},
    },
    thir_to_mir::mir,
    typecheck::type_check_function,
};
use std::collections::HashSet;

pub fn check_fundef(db: &dyn Db, fdef: FunctionId) {
    timed(
        db,
        Phase::Checking,
        || {
            checked_thir_body(db, fdef.interned());

            let mut checked = 0;
            for (fdef, subs) in reachable_mir_instances(db, fdef) {
                if fdef.has_body(db) && thir_is_valid(db, fdef) {
                    let the_mir = mir(db, fdef, subs);
                    let dce = dce(db, the_mir.func);
                    timed(
                        db,
                        Phase::BorrowCheck,
                        || check_mir(db, dce),
                        |()| Counts { functions: 1, stmts: None, exprs: None },
                    );
                    checked += 1;
                }
            }
            checked
        },
        |&checked| Counts { functions: checked, stmts: None, exprs: None },
    );
}

pub fn reachable_mir_instances(
    db: &dyn Db,
    root: FunctionId,
) -> Vec<(FunctionId, Substitution)> {
    let mut seen: HashSet<MIRKey> = HashSet::new();
    let mut worklist: Vec<(FunctionId, Substitution)> = vec![];
    let mut res = vec![];

    if !get_templates_of_fun(db, root.interned()).is_empty() {
        // Gneric so not a monomorphization root
        return res;
    }
    worklist.push((root, Substitution::empty()));

    while let Some((fdef, subs)) = worklist.pop() {
        if !seen.insert(MIRKey::new(db, fdef, subs.clone())) {
            continue;
        }
        res.push((fdef, subs.clone()));

        if !fdef.ast(db).has_body() {
            continue; // extern: no calls to walk
        }

        let Some(tc) = type_check_function(db, fdef) else {
            continue;
        };
        for call_info in tc.call_infos(db).values() {
            let Some(callee_subs) = call_info
                .substitution
                .iter()
                .map(|ty| ty.to_concrete(db, &subs))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let callee_subs = Substitution::new(callee_subs);

            assert_eq!(
                get_templates_of_fun(db, call_info.callee.into()).len(),
                callee_subs.len()
            );

            let zelf = match call_info.zelf_ty {
                Some(ty) => {
                    let Some(concrete) = ty.to_concrete(db, &subs) else { continue };
                    Some(concrete)
                }
                None => None,
            };

            let Some((fdef, callee_subs)) =
                concretize_fid(db, call_info.callee, &callee_subs, zelf)
            else {
                continue;
            };

            worklist.push((fdef, callee_subs));
        }
    }

    res
}
