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

use itertools::Itertools;

use crate::{
    Db,
    check::{
        mir::check_mir,
        thir::{checked_thir_body, thir_is_valid},
    },
    hir::function_ast,
    mir::passes::dead_code_elimination::dce,
    name_resolve::type_expr::get_templates_of_fun,
    resolved::{FunctionId, ScopeOwnerId, TypeRef},
    thir_to_mir::{MIRKey, mir},
    typecheck::{conformance::method_impl_for, type_check_function},
};
use std::collections::HashSet;

pub fn check_fundef(db: &dyn Db, fdef: FunctionId) {
    checked_thir_body(db, fdef.interned());

    for (fdef, subs) in reachable_mir_instances(db, fdef) {
        if fdef.has_body(db) && thir_is_valid(db, fdef) {
            let the_mir = mir(db, fdef, subs);
            let dce = dce(db, the_mir.func);
            check_mir(db, dce);
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
            let callee_subs: Vec<TypeRef> = call_info
                .substitution
                .iter()
                .map(|ty| ty.with_substitution(db, &subs))
                .collect();

            assert_eq!(
                get_templates_of_fun(db, call_info.callee.into()).len(),
                callee_subs.len()
            );

            let zelf = call_info.zelf_ty.map(|ty| ty.with_substitution(db, &subs));

            let (fdef, callee_subs) =
                concretize_fid(db, call_info.callee, &callee_subs, zelf);

            worklist.push((fdef, callee_subs));
        }
    }

    res
}

pub fn concretize_fid(
    db: &dyn Db,
    f_id: FunctionId,
    callee_subs: &[TypeRef],
    zelf: Option<TypeRef>,
) -> (FunctionId, Vec<TypeRef>) {
    let ScopeOwnerId::Interface(i_ref) = f_id.parent(db) else {
        return (f_id, callee_subs.to_vec());
    };
    let zelf = zelf.unwrap().as_type_id().unwrap();
    let is_static = f_id.receiver(db).is_static();
    let arity = f_id.args(db).1.len();
    let hint = Some(i_ref.def(db));
    let method =
        method_impl_for(db, zelf, f_id.name(db), arity, is_static, hint).unwrap();
    let mut new_subs = method.subs.iter().map(|ty| TypeRef::Concrete(*ty)).collect_vec();
    new_subs.extend(callee_subs);
    (method.method_id, new_subs)
}
