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

#[derive(Debug, Clone, Copy, Default)]
pub struct Subst<'a> {
    pub params: Option<&'a [TypeRef]>,
    pub zelf: Option<TypeRef>,
}

impl<'a> Subst<'a> {
    pub fn params(params: &'a [TypeRef]) -> Self {
        Self { params: Some(params), zelf: None }
    }

    pub fn zelf(zelf: TypeRef) -> Self {
        Self { params: None, zelf: Some(zelf) }
    }

    #[must_use]
    pub fn with_zelf(self, zelf: Option<TypeRef>) -> Self {
        Self { zelf, ..self }
    }
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
}

impl TryFrom<TypeRef> for TypeId {
    type Error = ();

    fn try_from(value: TypeRef) -> Result<Self, Self::Error> {
        value.as_type_id().ok_or(())
    }
}

pub trait TyLike: Sized {
    type Sub<'a>
    where
        Self: 'a;

    fn as_builtin<'a>(&'a self, db: &'a dyn Db) -> Option<(BuiltinTypeId, &'a [Self])>;

    fn sub(ty: &Self) -> Self::Sub<'_>;

    fn as_ptr_like<'a>(&'a self, db: &'a dyn Db) -> Option<(Mutability, Self::Sub<'a>)> {
        let (kind, inner) = ptr_like(self, db)?;
        Some((kind.mutability(), Self::sub(inner)))
    }

    fn as_ref<'a>(&'a self, db: &'a dyn Db) -> Option<(Mutability, Self::Sub<'a>)> {
        match ptr_like(self, db)? {
            (PtrKind::Ref(mutability), inner) => Some((mutability, Self::sub(inner))),
            (PtrKind::RawPtr(_), _) => None,
        }
    }

    fn as_ptr<'a>(&'a self, db: &'a dyn Db) -> Option<(Mutability, Self::Sub<'a>)> {
        match ptr_like(self, db)? {
            (PtrKind::RawPtr(mutability), inner) => Some((mutability, Self::sub(inner))),
            (PtrKind::Ref(_), _) => None,
        }
    }

    fn as_slice<'a>(&'a self, db: &'a dyn Db) -> Option<Self::Sub<'a>> {
        let (b, args) = self.as_builtin(db)?;
        matches!(b.kind(db), BuiltinTypeKind::Slice).then(|| Self::sub(&args[0]))
    }

    fn as_tuple<'a>(&'a self, db: &'a dyn Db) -> Option<&'a [Self]> {
        let (b, args) = self.as_builtin(db)?;
        matches!(b.kind(db), BuiltinTypeKind::Tuple).then_some(args)
    }
}

fn ptr_like<'a, T: TyLike>(ty: &'a T, db: &'a dyn Db) -> Option<(PtrKind, &'a T)> {
    let (b, args) = ty.as_builtin(db)?;
    Some((b.is_ptr_like(db)?, &args[0]))
}

impl TyLike for TypeRef {
    type Sub<'a> = Self;

    fn as_builtin<'a>(&'a self, db: &'a dyn Db) -> Option<(BuiltinTypeId, &'a [Self])> {
        let id = self.as_type_id()?;
        match id.def(db) {
            TypeDefId::Builtin(bid) => Some((bid, id.args(db))),
            _ => None,
        }
    }

    fn sub(ty: &Self) -> Self {
        *ty
    }
}

impl TypeRef {
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
            Some(usize_id(db).as_type_ref(db))
        } else {
            None
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
    pub fn subst(self, db: &dyn Db, subst: &Subst<'_>) -> Self {
        match self {
            Self::Concrete(id) => Self::Concrete(TypeId::new(
                db,
                id.def(db),
                id.args(db).iter().map(|t| t.subst(db, subst)).collect(),
            )),
            Self::Param(p) => match subst.params {
                Some(params) => params.get(p.0).copied().unwrap_or(Self::Error),
                None => self,
            },
            Self::Zelf => subst.zelf.unwrap_or(self),
            Self::Associated(_) | Self::Error | Self::Unknown => self,
        }
    }
}
