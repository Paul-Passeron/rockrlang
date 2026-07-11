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

use std::collections::HashMap;

use itertools::Itertools;

use crate::{
    Db,
    common::symbols::{InternedSymbol, Symbol},
    compiler::{Workspace, workspace_packages},
    name_resolve::implems::impls_in_package,
    ril::{
        FunctionId, ImplId, InterfaceId, InterfaceRef, InternedInterfaceRef,
        InternedTypeId, TypeId, TypeRef,
    },
};

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CandidateImpl {
    pub id: ImplId,
    pub subs: Vec<TypeId>,
}

fn _type_match(
    db: &dyn Db,
    a: TypeId,
    b: TypeRef,
    constraints: &mut HashMap<usize, TypeId>, // templates
) -> bool {
    match b {
        TypeRef::Concrete(b_id) => {
            if a.def(db) != b_id.def(db) {
                return false;
            }
            a.args(db).into_iter().zip_eq(b_id.args(db)).all(|(a, b)| {
                a.as_type_id()
                    .is_some_and(|a| _type_match(db, a, b, constraints))
            })
        }
        TypeRef::Param(id) => match constraints.get(&id.0).cloned() {
            Some(prev_a_matching_template) => {
                if a == prev_a_matching_template {
                    return true;
                }
                if a.def(db) != prev_a_matching_template.def(db) {
                    return false;
                }
                a.args(db)
                    .into_iter()
                    .zip_eq(prev_a_matching_template.args(db))
                    .all(|(a, b)| {
                        a.as_type_id()
                            .is_some_and(|a| _type_match(db, a, b, constraints))
                    })
            }
            None => {
                constraints.insert(id.0, a);
                true
            }
        },
        TypeRef::Zelf => todo!(),
        _ => false,
    }
}

fn type_match(db: &dyn Db, a: TypeId, b: TypeRef) -> Option<Vec<TypeId>> {
    let mut m = HashMap::new();
    if _type_match(db, a, b, &mut m) { todo!() } else { None }
}

#[salsa::tracked(returns(ref))]
fn _candidate_impls_for<'db>(
    db: &'db dyn Db,
    ty: InternedTypeId<'db>,
) -> Vec<CandidateImpl> {
    let ws = Workspace::get(db);
    let packages = workspace_packages(db, ws);
    let ty: TypeId = ty.into();
    packages
        .iter()
        .flat_map(|pkg| impls_in_package(db, *pkg))
        .map(|src| src.id(db))
        .unique()
        .filter_map(|id| {
            let implemented_ty = id.implemented(db);
            let subs = type_match(db, ty, implemented_ty)?;
            Some(CandidateImpl { id, subs })
        })
        .collect()
}

pub fn candidate_impls_for(db: &dyn Db, ty: TypeId) -> &[CandidateImpl] {
    _candidate_impls_for(db, ty.into())
}

fn _type_implements_initial(
    _db: &dyn Db,
    _id: salsa::Id,
    _ty: InternedTypeId<'_>,
    _interface: InternedInterfaceRef<'_>,
) -> Option<ImplId> {
    None
}

#[salsa::tracked(cycle_initial=_type_implements_initial)]
fn _type_implements<'db>(
    db: &'db dyn Db,
    ty: InternedTypeId<'db>,
    interface: InternedInterfaceRef<'db>,
) -> Option<ImplId> {
    let impls = candidate_impls_for(db, ty.into());
    impls.iter().find_map(|impl_id| {
        let id = impl_id.id;
        let subs = &impl_id.subs;
        if id.interface(db) != Some(interface.into()) {
            return None;
        }
        let templs = id.templates(db);
        if subs.len() != templs.len() {
            // This is a bug
            return None;
        }
        let templ_matches = templs.iter().zip(subs).all(|(interfaces, ty)| {
            interfaces
                .iter()
                .all(|interface| type_implements(db, *ty, *interface).is_some())
        });
        if templ_matches { Some(id) } else { None }
    })
}

pub fn type_implements(
    db: &dyn Db,
    ty: TypeId,
    interface: InterfaceRef,
) -> Option<ImplId> {
    _type_implements(db, ty.into(), interface.into())
}

#[salsa::tracked]
fn _method_impl_for<'db>(
    db: &'db dyn Db,
    ty: InternedTypeId<'db>,
    method: InternedSymbol<'db>,
    arity: usize,
    is_static: bool,
    hint: Option<InterfaceId>,
) -> Option<(ImplId, FunctionId)> {
    todo!()
}

pub fn method_impl_for<'db>(
    db: &'db dyn Db,
    ty: TypeId,
    method: Symbol,
    arity: usize,
    is_static: bool,
    hint: Option<InterfaceId>,
) -> Option<(ImplId, FunctionId)> {
    _method_impl_for(db, ty.into(), method.interned(), arity, is_static, hint)
}
