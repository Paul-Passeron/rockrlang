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
    ril::{
        ImplId, InterfaceRef, InternedInterfaceRef, InternedTypeId, TypeId,
        TypeRef,
    },
};

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CandidateImpl {
    pub id: ImplId,
    pub subs: Vec<TypeRef>,
}

#[salsa::tracked(returns(ref))]
fn _candidate_impls_for<'db>(
    db: &'db dyn Db,
    ty: InternedTypeId<'db>,
) -> Vec<CandidateImpl> {
    todo!()
}

pub fn candidate_impls_for(db: &dyn Db, ty: TypeId) -> &[CandidateImpl] {
    _candidate_impls_for(db, ty.into())
}

#[salsa::tracked]
fn _type_implements<'db>(
    db: &'db dyn Db,
    ty: InternedTypeId<'db>,
    interface: InternedInterfaceRef<'db>,
) -> Option<ImplId> {
    todo!()
}

pub fn type_implements(
    db: &dyn Db,
    ty: TypeId,
    interface: InterfaceRef,
) -> Option<ImplId> {
    _type_implements(db, ty.into(), interface.into())
}



