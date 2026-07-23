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
    hir::impl_items,
    name_resolve::implems::impls_in_package,
    parse_tree::top_level::AstImplItem,
    resolved::{
        FunctionId, ImplId, InterfaceId, InterfaceRef, InternedInterfaceRef,
        InternedTypeId, ScopeOwnerId, TypeId, TypeRef,
    },
};

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CandidateImpl {
    pub id: ImplId,

    pub subs: Vec<Option<TypeId>>,
}

fn _type_match(
    db: &dyn Db,
    a: TypeId,
    b: TypeRef,
    zelf: Option<TypeId>,
    constraints: &mut HashMap<usize, TypeId>, // templates
) -> bool {
    match b {
        TypeRef::Concrete(b_id) => {
            if a.def(db) != b_id.def(db) {
                return false;
            }
            let a_args = a.args(db);
            let b_args = b_id.args(db);
            if a_args.len() != b_args.len() {
                // Problem
                return false;
            }
            a_args.iter().zip(b_args).all(|(a, b)| {
                a.as_type_id().is_some_and(|a| _type_match(db, a, *b, zelf, constraints))
            })
        }
        TypeRef::Param(id) => match constraints.get(&id.0) {
            Some(prev) => a == *prev,
            None => {
                constraints.insert(id.0, a);
                true
            }
        },
        TypeRef::Zelf => zelf.is_some_and(|z| a == z),
        _ => false,
    }
}

fn type_match(
    db: &dyn Db,
    a: TypeId,
    b: TypeRef,
    n_templates: usize,
) -> Option<Vec<Option<TypeId>>> {
    let mut m = HashMap::new();
    if _type_match(db, a, b, None, &mut m) {
        Some((0..n_templates).map(|i| m.get(&i).copied()).collect())
    } else {
        None
    }
}

fn type_ref_is_concrete(db: &dyn Db, ty: TypeRef) -> bool {
    match ty {
        TypeRef::Concrete(id) => type_id_is_concrete(db, id),
        _ => false,
    }
}

fn type_id_is_concrete(db: &dyn Db, ty: TypeId) -> bool {
    ty.args(db).iter().all(|arg| type_ref_is_concrete(db, *arg))
}

#[salsa::tracked]
fn _candidate_impls_for<'db>(
    db: &'db dyn Db,
    ty: InternedTypeId<'db>,
) -> Vec<CandidateImpl> {
    let ws = Workspace::get(db);
    let packages = workspace_packages(db, ws);
    let ty: TypeId = ty.into();
    packages
        .iter()
        .flat_map(|pkg| impls_in_package(db, *pkg).iter())
        .map(|src| src.id(db))
        .unique()
        .filter_map(|id| {
            let id = *id;
            let implemented_ty = id.implemented(db);
            let n_templates = id.templates(db).len();
            let subs = type_match(db, ty, implemented_ty, n_templates)?;
            Some(CandidateImpl { id, subs })
        })
        .collect()
}

pub fn candidate_impls_for(db: &dyn Db, ty: TypeId) -> &[CandidateImpl] {
    debug_assert!(
        type_id_is_concrete(db, ty),
        "candidate_impls_for called with a non-concrete key"
    );
    _candidate_impls_for(db, ty.into())
}

fn impl_bounds_hold(db: &dyn Db, id: ImplId, subs: &[TypeId], zelf: TypeId) -> bool {
    let templs = id.templates(db);
    debug_assert_eq!(subs.len(), templs.len());
    let sub_refs = subs.iter().map(|ty| TypeRef::Concrete(*ty)).collect_vec();
    templs.iter().zip(subs).all(|(interfaces, ty)| {
        interfaces.iter().all(|interface| {
            let bound = iref_sub(db, *interface, &sub_refs, TypeRef::Concrete(zelf));
            type_implements(db, *ty, bound).is_some()
        })
    })
}

fn _type_implements_initial(
    _db: &dyn Db,
    _id: salsa::Id,
    _ty: InternedTypeId<'_>,
    _interface: InternedInterfaceRef<'_>,
) -> Option<ImplId> {
    None
}

#[salsa::tracked(
    returns(copy),
    cycle_initial=_type_implements_initial
)]
fn _type_implements<'db>(
    db: &'db dyn Db,
    ty: InternedTypeId<'db>,
    interface: InternedInterfaceRef<'db>,
) -> Option<ImplId> {
    let self_ty: TypeId = ty.into();
    let requested: InterfaceRef = interface.into();
    // note: first implem matching wins here
    candidate_impls_for(db, self_ty).iter().find_map(|candidate| {
        let id = candidate.id;
        let iref = id.interface(db)?; // Do we implement an interface ?
        if iref.def(db) != requested.def(db) {
            return None;
        }

        let mut m: HashMap<usize, TypeId> = candidate
            .subs
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.map(|s| (i, s)))
            .collect();
        let req_args = requested.args(db);
        let pat_args = iref.args(db);
        if req_args.len() != pat_args.len() {
            // Problem
            return None;
        }
        for (req, pat) in req_args.iter().zip(pat_args) {
            let req = req.as_type_id()?;
            if !_type_match(db, req, *pat, Some(self_ty), &mut m) {
                return None;
            }
        }

        let n_templates = id.templates(db).len();
        let subs: Option<Vec<TypeId>> =
            (0..n_templates).map(|i| m.get(&i).copied()).collect();
        let subs = subs?;

        if !impl_bounds_hold(db, id, &subs, self_ty) {
            return None;
        }
        Some(id)
    })
}

pub fn type_implements(
    db: &dyn Db,
    ty: TypeId,
    interface: InterfaceRef,
) -> Option<ImplId> {
    debug_assert!(
        type_id_is_concrete(db, ty)
            && interface.args(db).iter().all(|arg| type_ref_is_concrete(db, *arg)),
        "type_implements called with a non-concrete key"
    );
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
) -> Option<MethodImpl> {
    let method: Symbol = method.into();
    let self_ty: TypeId = ty.into();
    candidate_impls_for(db, self_ty).iter().find_map(|candidate| {
        let id = candidate.id;
        if let Some(hint) = hint {
            let iref = id.interface(db)?;
            if iref.def(db) != hint {
                return None;
            }
        }

        let subs: Option<Vec<TypeId>> = candidate.subs.iter().copied().collect();
        let subs = subs?;
        if !impl_bounds_hold(db, id, &subs, self_ty) {
            return None;
        }
        let fid = impl_items(db, id.into()).iter().find_map(|item| match item {
            AstImplItem::Fundef(ast) => {
                if ast.data.name.data != method {
                    return None;
                }
                if ast.data.receiver.is_static() != is_static {
                    return None;
                }
                if ast.data.args.len() != arity {
                    return None;
                }
                Some(FunctionId::new(db, method, ScopeOwnerId::Impl(id)))
            }
            _ => None,
        })?;
        Some(MethodImpl { impl_id: id, method_id: fid, subs })
    })
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct MethodImpl {
    pub impl_id: ImplId,
    pub method_id: FunctionId,
    pub subs: Vec<TypeId>,
}

pub fn method_impl_for(
    db: &dyn Db,
    ty: TypeId,
    method: Symbol,
    arity: usize,
    is_static: bool,
    hint: Option<InterfaceId>,
) -> Option<&MethodImpl> {
    debug_assert!(
        type_id_is_concrete(db, ty),
        "method_impl_for called with a non-concrete key"
    );
    _method_impl_for(db, ty.into(), method.interned(), arity, is_static, hint).as_ref()
}

fn sub(db: &dyn Db, ty: TypeRef, subs: &[TypeRef], zelf: TypeRef) -> TypeRef {
    match ty {
        TypeRef::Concrete(type_id) => TypeRef::Concrete(TypeId::new(
            db,
            type_id.def(db),
            type_id.args(db).iter().map(|ty| sub(db, *ty, subs, zelf)).collect(),
        )),
        TypeRef::Param(id) => subs[id.0],
        TypeRef::Associated(_) => todo!(),
        TypeRef::Zelf => zelf,
        TypeRef::Error => TypeRef::Error,
        TypeRef::Unknown => TypeRef::Unknown,
    }
}

fn iref_sub(
    db: &dyn Db,
    iref: InterfaceRef,
    subs: &[TypeRef],
    zelf: TypeRef,
) -> InterfaceRef {
    InterfaceRef::new(
        db,
        iref.def(db),
        iref.args(db).iter().map(|ty| sub(db, *ty, subs, zelf)).collect(),
    )
}
