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

use rockr_base::{Db, mutability::Mutability, symbol::Symbol};

use crate::{
    resolved::{
        AdtLike, BuiltinTypeId, BuiltinTypeKind, EnumId, PtrKind, ScopeOwnerId, StructId,
        TypeDefId, TypeId, TypeRef, ref_of, slice_of, type_printer::TypePrinter,
        usize_id,
    },
    typecheck::inference::implicit::{AsAstImplCtx, AstImplicitContext},
};
use rockr_syntax::parse_tree::top_level::AstEnumVariantKind;

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

    fn as_array<'a>(&'a self, db: &'a dyn Db) -> Option<(Self::Sub<'a>, usize)> {
        let (b, args) = self.as_builtin(db)?;
        match b.kind(db) {
            BuiltinTypeKind::Array { length } => Some((Self::sub(&args[0]), length)),
            _ => None,
        }
    }

    fn as_slice<'a>(&'a self, db: &'a dyn Db) -> Option<(Self::Sub<'a>, Mutability)> {
        let (b, args) = self.as_builtin(db)?;
        match b.kind(db) {
            BuiltinTypeKind::Slice { mutability } => {
                Some((Self::sub(&args[0]), mutability))
            }
            _ => None,
        }
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
            .or_else(|| self.as_array(db).map(|ty| ty.0))
            .or_else(|| self.as_slice(db).map(|ty| ty.0))
            .or_else(|| self.as_ref(db).and_then(|t| t.1.as_array(db).map(|ty| ty.0)))
    }

    // TODO: What was the point of this, exactly ???
    pub fn ref_slice_of(db: &dyn Db, inner: Self) -> Self {
        Self::Concrete(slice_of(db, inner, Mutability::Const))
    }

    pub fn typeof_metadata(&self, db: &dyn Db) -> Option<Self> {
        self.is_fat_ptr(db).then(|| usize_id(db).as_type_ref(db))
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

pub enum ConstructorType {
    Tuple(Vec<TypeRef>),
    Struct(Vec<(Symbol, TypeRef)>),
    None,
}

impl ConstructorType {
    #[must_use]
    pub fn with_substitution(&self, db: &dyn Db, sub: &[TypeRef]) -> Self {
        match self {
            Self::Tuple(type_refs) => Self::Tuple(
                type_refs.iter().map(|ty| ty.subst(db, &Subst::params(sub))).collect(),
            ),
            Self::Struct(items) => Self::Struct(
                items
                    .iter()
                    .map(|(name, ty)| (*name, ty.subst(db, &Subst::params(sub))))
                    .collect(),
            ),
            Self::None => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapKind {
    Ref(Mutability),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefWrappedTy<T: AdtLike> {
    pub inner: T,
    pub refs: Vec<WrapKind>, // [&A, &B, &C] means &C &B &A T
}

impl<T: AdtLike> RefWrappedTy<T> {
    pub fn wrap_like(&self, db: &dyn Db, to_wrap: T) -> T {
        self.refs.iter().fold(to_wrap, |ty, ref_kind| ref_kind.wrap(db, ty.into()))
    }

    pub fn as_type_ref(&self, db: &dyn Db) -> T {
        self.wrap_like(db, self.inner)
    }

    pub fn peel_until(db: &dyn Db, to_peel: T, target: T) -> Option<Self> {
        let mut refs = vec![];
        let mut inner = to_peel;
        while inner != target
            && let Some((mutability, ty)) = inner.peel_ref(db)
        {
            inner = ty.try_into().ok()?;
            refs.push(WrapKind::Ref(mutability));
        }
        if inner != target {
            return None;
        }
        Some(Self { inner, refs })
    }

    pub fn from_type(db: &dyn Db, ty: T) -> Self {
        let mut refs = vec![];
        let mut inner = ty;
        while let Some((mutability, ty)) = inner.peel_ref(db) {
            inner = ty.try_into().ok().unwrap();
            refs.push(WrapKind::Ref(mutability));
        }
        refs.reverse();
        Self { inner, refs }
    }

    pub fn depth(&self) -> usize {
        self.refs.len()
    }
}

impl WrapKind {
    pub fn wrap<T: AdtLike>(self, db: &dyn Db, ty: T::Args) -> T {
        let Self::Ref(mutability) = self;
        ref_of(db, ty, mutability.is_mut())
    }
}

impl TypeRef {
    pub fn contains_error(&self, db: &dyn Db) -> bool {
        match self {
            Self::Error => true,
            Self::Concrete(id) => id.args(db).iter().any(|arg| arg.contains_error(db)),
            Self::Param(_) | Self::Associated(_) | Self::Zelf | Self::Unknown => false,
        }
    }
}

/// Applies `refs` (as collected by [`peel_type_ref_until`]) on top of `ty`.
///
/// Unlike [`RefWrappedTy::wrap_like`], this works directly on `TypeRef` and so
/// tolerates non-concrete leaves (e.g. a bare `Param` inside a generic
/// function's own body), which `RefWrappedTy<TypeId>` cannot represent.
pub fn wrap_type_ref(db: &dyn Db, refs: &[WrapKind], ty: TypeRef) -> TypeRef {
    refs.iter()
        .fold(ty, |ty, WrapKind::Ref(mutability)| ty.wrap_ref(db, mutability.is_mut()))
}

/// Like [`RefWrappedTy`], but over `TypeRef` directly — `AdtLike` cannot be
/// implemented for `TypeRef` since a bare `Param`/`Zelf`/`Error`/`Unknown` has
/// no `def()`, and those legitimately appear here in unmonomorphized THIR.
pub struct PeeledTy {
    pub inner: TypeRef,
    pub refs: Vec<WrapKind>,
}

impl PeeledTy {
    pub fn from_type_ref(db: &dyn Db, mut ty: TypeRef) -> Self {
        let mut refs = vec![];
        while let Some((mutability, inner)) = ty.as_ref(db) {
            ty = inner;
            refs.push(WrapKind::Ref(mutability));
        }
        refs.reverse();
        Self { inner: ty, refs }
    }
}

pub fn peel_type_ref_until(
    db: &dyn Db,
    mut ty: TypeRef,
    target: TypeRef,
) -> Option<PeeledTy> {
    let mut refs = vec![];
    while ty != target
        && let Some((mutability, inner)) = ty.as_ref(db)
    {
        ty = inner;
        refs.push(WrapKind::Ref(mutability));
    }
    if ty != target {
        return None;
    }
    Some(PeeledTy { inner: ty, refs })
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructRef {
    pub def: StructId,
    pub args: Vec<TypeRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnumRef {
    pub def: EnumId,
    pub args: Vec<TypeRef>,
}

impl EnumRef {
    pub fn into_type_ref(self, db: &dyn Db) -> TypeRef {
        TypeId::new(db, TypeDefId::Enum(self.def), self.args).into()
    }
}

impl StructRef {
    pub fn into_type_ref(self, db: &dyn Db) -> TypeRef {
        TypeId::new(db, TypeDefId::Struct(self.def), self.args).into()
    }
}

impl EnumRef {
    pub fn get_cons(&self, db: &dyn Db, idx: usize) -> Option<ConstructorType> {
        let item = self.def.item(db);
        let variant = item.variants.get(idx)?;
        let raw = match &variant.kind {
            AstEnumVariantKind::Unit => Some(ConstructorType::None),
            AstEnumVariantKind::StructLike(fields) => {
                let ctx = AstImplicitContext::new(
                    db,
                    ScopeOwnerId::Module(self.def.parent(db)),
                    &item.template_args,
                );
                Some(ConstructorType::Struct(
                    fields
                        .iter()
                        .map(|field| (field.name, ctx.resolve_err(db, &field.ty.data)))
                        .collect(),
                ))
            }
            AstEnumVariantKind::TupleLike(spanneds) => {
                let ctx = AstImplicitContext::new(
                    db,
                    ScopeOwnerId::Module(self.def.parent(db)),
                    &item.template_args,
                );
                Some(ConstructorType::Tuple(
                    spanneds.iter().map(|ast| ctx.resolve_err(db, &ast.data)).collect(),
                ))
            }
        }?;
        Some(raw.with_substitution(db, &self.args))
    }

    pub fn variants(&self, db: &dyn Db) -> Vec<ConstructorType> {
        let n = self.def.item(db).variants.len();
        (0..n).into_iter().map(|i| self.get_cons(db, i).unwrap()).collect()
    }
}

impl StructRef {
    /// Field types as declared on the struct definition, still referencing the
    /// struct's own template parameters rather than the concrete `self.args`.
    pub fn declared_fields_ty(&self, db: &dyn Db) -> HashMap<Symbol, TypeRef> {
        let item = self.def.item(db);
        let mut res = HashMap::new();
        for field in &item.fields {
            let ctx = AstImplicitContext::new(
                db,
                ScopeOwnerId::Module(self.def.parent(db)),
                &item.template_args,
            );
            res.insert(field.name, ctx.resolve_err(db, &field.ty.data));
        }
        res
    }

    pub fn get_fields_ty(&self, db: &dyn Db) -> HashMap<Symbol, TypeRef> {
        self.declared_fields_ty(db)
            .into_iter()
            .map(|(name, ty)| (name, ty.subst(db, &Subst::params(&self.args))))
            .collect()
    }
}
