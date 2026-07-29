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

//! Structural accessors and constructors on [`TypeRef`] — the low-level query
//! API used throughout the pipeline (peeling refs/ptrs/slices, substituting
//! params, wrapping in references, etc.).

use super::{
    BuiltinTypeId, BuiltinTypeKind, PtrKind, TypeDefId, TypeId, TypeRef, ref_of,
    slice_of, usize_id,
};
use crate::{Db, hir::Mutability, printer::type_printer::TypePrinter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastClass {
    Int,
    ThinPtr(Mutability),
    FatPtr(Mutability),
}

impl TypeRef {
    pub fn to_string(self, db: &dyn Db) -> String {
        TypePrinter::new().type_ref_to_string(db, self)
    }

    pub fn as_type_id(self) -> Option<TypeId> {
        match self {
            Self::Concrete(type_id) => Some(type_id),
            _ => None,
        }
    }

    pub fn as_builtin(self, db: &dyn Db) -> Option<(BuiltinTypeId, &[Self])> {
        let id = self.as_type_id()?;
        match id.def(db) {
            TypeDefId::Builtin(bid) => Some((bid, id.args(db))),
            _ => None,
        }
    }

    pub fn as_tuple_ref(self, db: &dyn Db) -> Option<Vec<Self>> {
        let (b, args) = self.as_builtin(db)?;
        matches!(b.kind(db), BuiltinTypeKind::Tuple).then(|| args.to_vec())
    }

    pub fn as_ref(self, db: &dyn Db) -> Option<(Mutability, Self)> {
        let type_id = self.as_type_id()?;
        let ptr_kind = type_id.def(db).is_ptr_like(db)?;
        match ptr_kind {
            PtrKind::Ref(mutability) => Some((mutability, type_id.args(db)[0])),
            PtrKind::RawPtr(_) => None,
        }
    }

    pub fn as_ptr(self, db: &dyn Db) -> Option<(Mutability, Self)> {
        let type_id = self.as_type_id()?;
        let ptr_kind = type_id.def(db).is_ptr_like(db)?;
        match ptr_kind {
            PtrKind::RawPtr(mutability) => Some((mutability, type_id.args(db)[0])),
            PtrKind::Ref(_) => None,
        }
    }

    pub fn as_ptr_like(self, db: &dyn Db) -> Option<(Mutability, Self)> {
        let type_id = self.as_type_id()?;
        let mutability = type_id.def(db).is_ptr_like(db)?.mutability();
        Some((mutability, type_id.args(db)[0]))
    }

    pub fn cast_class(self, db: &dyn Db) -> Option<CastClass> {
        if let Some((muta, _)) = self.as_ptr_like(db) {
            return Some(if self.is_fat_ptr(db) {
                CastClass::FatPtr(muta)
            } else {
                CastClass::ThinPtr(muta)
            });
        }
        let (builtin, _) = self.as_builtin(db)?;
        matches!(builtin.kind(db), BuiltinTypeKind::Bool | BuiltinTypeKind::Int { .. })
            .then_some(CastClass::Int)
    }

    pub fn as_slice(self, db: &dyn Db) -> Option<Self> {
        let (b, args) = self.as_builtin(db)?;
        match b.kind(db) {
            BuiltinTypeKind::Slice => Some(args[0]),
            _ => None,
        }
    }

    pub fn element_of_indexed(self, db: &dyn Db) -> Option<Self> {
        self.as_ptr(db)
            .map(|(_, inner)| inner)
            .or_else(|| self.as_slice(db))
            .or_else(|| self.as_ref(db).and_then(|t| t.1.as_slice(db)))
    }

    pub fn ref_slice_of(db: &dyn Db, inner: Self) -> Self {
        Self::Concrete(slice_of(db, Self::Concrete(slice_of(db, inner))))
    }

    pub fn typeof_metadata(&self, db: &dyn Db) -> Option<Self> {
        if self.as_ref(db).and_then(|(_, ty)| ty.as_slice(db)).is_some() {
            Some(Self::Concrete(usize_id(db)))
        } else {
            None
        }
    }

    #[must_use]
    pub fn with_substitution(self, db: &dyn Db, sub: &[Self]) -> Self {
        match self {
            Self::Concrete(type_id) => Self::Concrete(TypeId::new(
                db,
                type_id.def(db),
                type_id.args(db).iter().map(|t| t.with_substitution(db, sub)).collect(),
            )),
            Self::Param(id) => sub[id.0],
            Self::Associated(_) | Self::Zelf | Self::Error | Self::Unknown => self,
        }
    }

    #[must_use]
    pub fn wrap_ref(self, db: &dyn Db, mutable: bool) -> Self {
        match self {
            Self::Error | Self::Unknown => self,
            _ => Self::Concrete(ref_of(db, self, mutable)),
        }
    }

    #[must_use]
    pub fn instantiate(self, db: &dyn Db, subs: &[Self], zelf: Option<Self>) -> Self {
        match self {
            Self::Concrete(id) => Self::Concrete(TypeId::new(
                db,
                id.def(db),
                id.args(db).iter().map(|t| t.instantiate(db, subs, zelf)).collect(),
            )),
            Self::Param(p) => subs[p.0], // callee-space param
            Self::Zelf => {
                zelf.expect("Zelf in signature but no self type on FunctionRef")
            }
            other => other,
        }
    }
}
