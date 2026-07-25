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

use nonempty::nonempty;
use std::marker::PhantomData;

use crate::{
    hir::Mutability,
    layout::Size,
    name_resolve::{
        core_module,
        definition::{Definition, Segments, resolve_path},
    },
};

use super::*;

impl<'db> From<InternedModuleId<'db>> for ModuleId {
    fn from(v: InternedModuleId<'db>) -> Self {
        ModuleId(v.0)
    }
}

impl<'db> From<ModuleId> for InternedModuleId<'db> {
    fn from(v: ModuleId) -> Self {
        Self(v.0, PhantomData)
    }
}

impl ModuleId {
    pub fn new<'db>(
        db: &'db dyn Db,
        name: Symbol,
        parent: Option<ModuleId>,
        file: Option<SourceFile>,
        file_submodules: Vec<FileModule<'db>>,
        package: Option<Package<'db>>,
    ) -> Self {
        InternedModuleId::new(db, name, parent, file, file_submodules, package).into()
    }

    pub fn interned(self) -> InternedModuleId<'static> {
        InternedModuleId(self.0, PhantomData)
    }

    pub fn name(self, db: &dyn Db) -> Symbol {
        *self.interned().name(db)
    }

    pub fn parent(self, db: &dyn Db) -> Option<ModuleId> {
        *self.interned().parent(db)
    }

    pub fn file_submodules(self, db: &dyn Db) -> &[FileModule<'_>] {
        self.interned().file_submodules(db)
    }

    pub fn package(self, db: &dyn Db) -> Option<Package<'_>> {
        *self.interned().package(db)
    }

    pub fn owning_package(self, db: &dyn Db) -> Option<Package<'_>> {
        self.package(db)
            .or_else(|| self.parent(db).and_then(|parent| parent.owning_package(db)))
    }
}

impl<'db> From<InternedFunctionId<'db>> for FunctionId {
    fn from(v: InternedFunctionId<'db>) -> Self {
        FunctionId(v.0)
    }
}

impl<'db> From<FunctionId> for InternedFunctionId<'db> {
    fn from(v: FunctionId) -> Self {
        Self(v.0, PhantomData)
    }
}

#[allow(dead_code)]
impl FunctionId {
    pub fn new(db: &dyn Db, name: Symbol, parent: ScopeOwnerId) -> Self {
        InternedFunctionId::new(db, name, parent).into()
    }

    pub fn interned(self) -> InternedFunctionId<'static> {
        InternedFunctionId(self.0, PhantomData)
    }

    pub fn name(self, db: &dyn Db) -> Symbol {
        *self.interned().name(db)
    }

    pub fn parent(self, db: &dyn Db) -> ScopeOwnerId {
        *self.interned().parent(db)
    }
}

impl<'db> From<InternedStructId<'db>> for StructId {
    fn from(v: InternedStructId<'db>) -> Self {
        StructId(v.0)
    }
}

impl<'db> From<StructId> for InternedStructId<'db> {
    fn from(v: StructId) -> Self {
        Self(v.0, PhantomData)
    }
}

impl StructId {
    pub fn new(db: &dyn Db, name: Symbol, parent: ModuleId) -> Self {
        InternedStructId::new(db, name, parent).into()
    }

    pub fn interned(self) -> InternedStructId<'static> {
        InternedStructId(self.0, PhantomData)
    }

    pub fn name(self, db: &dyn Db) -> Symbol {
        *self.interned().name(db)
    }

    pub fn parent(self, db: &dyn Db) -> ModuleId {
        *self.interned().parent(db)
    }
}

impl<'db> From<InternedImplId<'db>> for ImplId {
    fn from(v: InternedImplId<'db>) -> Self {
        ImplId(v.0)
    }
}

impl<'db> From<ImplId> for InternedImplId<'db> {
    fn from(v: ImplId) -> Self {
        Self(v.0, PhantomData)
    }
}

#[allow(dead_code)]
impl ImplId {
    pub fn new(
        db: &dyn Db,
        parent: ModuleId,
        implemented: TypeRef,
        interface: Option<InterfaceRef>,
        templates: Vec<Set<InterfaceRef>>,
    ) -> Self {
        InternedImplId::new(db, parent, implemented, interface, templates).into()
    }

    pub fn interned(self) -> InternedImplId<'static> {
        InternedImplId(self.0, PhantomData)
    }

    pub fn parent(self, db: &dyn Db) -> ModuleId {
        *self.interned().parent(db)
    }

    pub fn implemented(self, db: &dyn Db) -> TypeRef {
        *self.interned().implemented(db)
    }

    pub fn interface(self, db: &dyn Db) -> Option<InterfaceRef> {
        *self.interned().interface(db)
    }

    pub fn templates(self, db: &dyn Db) -> &[Set<InterfaceRef>] {
        self.interned().templates(db)
    }
}

impl<'db> From<InternedInterfaceId<'db>> for InterfaceId {
    fn from(v: InternedInterfaceId<'db>) -> Self {
        InterfaceId(v.0)
    }
}

impl<'db> From<InterfaceId> for InternedInterfaceId<'db> {
    fn from(v: InterfaceId) -> Self {
        Self(v.0, PhantomData)
    }
}

impl InterfaceId {
    pub fn new(db: &dyn Db, name: Symbol, parent: ModuleId) -> Self {
        InternedInterfaceId::new(db, name, parent).into()
    }

    pub fn interned(self) -> InternedInterfaceId<'static> {
        InternedInterfaceId(self.0, PhantomData)
    }

    pub fn name(self, db: &dyn Db) -> Symbol {
        *self.interned().name(db)
    }

    pub fn parent(self, db: &dyn Db) -> ModuleId {
        *self.interned().parent(db)
    }
}

impl<'db> From<InternedTypeId<'db>> for TypeId {
    fn from(v: InternedTypeId<'db>) -> Self {
        TypeId(v.0)
    }
}

impl<'db> From<TypeId> for InternedTypeId<'db> {
    fn from(v: TypeId) -> Self {
        Self(v.0, PhantomData)
    }
}

impl TypeId {
    pub fn new(db: &dyn Db, def: TypeDefId, args: Vec<TypeRef>) -> Self {
        InternedTypeId::new(db, def, args).into()
    }

    pub fn interned(self) -> InternedTypeId<'static> {
        InternedTypeId(self.0, PhantomData)
    }

    pub fn def(self, db: &dyn Db) -> TypeDefId {
        *self.interned().def(db)
    }

    pub fn args(self, db: &dyn Db) -> &[TypeRef] {
        self.interned().args(db)
    }
}

impl From<BuiltinTypeId> for TypeDefId {
    fn from(value: BuiltinTypeId) -> Self {
        Self::Builtin(value)
    }
}

impl From<TypeId> for TypeRef {
    fn from(value: TypeId) -> Self {
        Self::Concrete(value)
    }
}

impl<'a> From<BuiltinTypeDef<'a>> for BuiltinTypeId {
    fn from(value: BuiltinTypeDef) -> Self {
        Self(value.0)
    }
}

impl<'db> BuiltinTypeDef<'db> {
    fn name(self, db: &dyn Db) -> Symbol {
        let as_str = match *self.kind(db) {
            BuiltinTypeKind::Void => "void".into(),
            BuiltinTypeKind::Never => "never".into(),
            BuiltinTypeKind::Bool => "bool".into(),
            BuiltinTypeKind::Int { width, signed } => {
                format!("{}{}", if signed { "i" } else { "u" }, {
                    let s: Size = width.into();
                    s.bytes() * 8
                })
            }
            BuiltinTypeKind::Ref { mutability } => {
                format!("&{}", if mutability.is_mut() { "mut" } else { "" })
            }
            BuiltinTypeKind::Ptr { mutability } => {
                format!("*{}", if mutability.is_mut() { "mut" } else { "" })
            }
            BuiltinTypeKind::Slice => "[]".into(),
            BuiltinTypeKind::Tuple => "()".into(),
        };
        Symbol::new(db, as_str)
    }
}

impl BuiltinTypeId {
    pub fn name(self, db: &dyn Db) -> Symbol {
        self.interned().name(db)
    }
}

impl BuiltinTypeId {
    pub fn interned<'a>(self) -> BuiltinTypeDef<'a> {
        BuiltinTypeDef(self.0, PhantomData)
    }

    pub fn ptr(db: &dyn Db, mutability: Mutability) -> Self {
        BuiltinTypeDef::new(db, BuiltinTypeKind::Ptr { mutability }).into()
    }

    pub fn const_ptr(db: &dyn Db) -> Self {
        Self::ptr(db, Mutability::Const)
    }
    pub fn mut_ptr(db: &dyn Db) -> Self {
        Self::ptr(db, Mutability::Mutable)
    }

    pub fn ref_(db: &dyn Db, mutability: Mutability) -> Self {
        BuiltinTypeDef::new(db, BuiltinTypeKind::Ref { mutability }).into()
    }
    pub fn const_ref(db: &dyn Db) -> Self {
        Self::ref_(db, Mutability::Const)
    }
    pub fn mut_ref(db: &dyn Db) -> Self {
        Self::ref_(db, Mutability::Mutable)
    }

    pub fn tuple(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(db, BuiltinTypeKind::Tuple).into()
    }

    pub fn slice(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(db, BuiltinTypeKind::Slice).into()
    }

    pub fn i8(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(
            db,
            BuiltinTypeKind::Int { width: IntWidth::I8, signed: true },
        )
        .into()
    }

    pub fn i16(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(
            db,
            BuiltinTypeKind::Int { width: IntWidth::I16, signed: true },
        )
        .into()
    }

    pub fn i32(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(
            db,
            BuiltinTypeKind::Int { width: IntWidth::I32, signed: true },
        )
        .into()
    }

    pub fn i64(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(
            db,
            BuiltinTypeKind::Int { width: IntWidth::I64, signed: true },
        )
        .into()
    }

    pub fn i128(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(
            db,
            BuiltinTypeKind::Int { width: IntWidth::I64, signed: true },
        )
        .into()
    }

    pub fn u8(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(
            db,
            BuiltinTypeKind::Int { width: IntWidth::I8, signed: false },
        )
        .into()
    }

    pub fn u16(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(
            db,
            BuiltinTypeKind::Int { width: IntWidth::I16, signed: false },
        )
        .into()
    }

    pub fn u32(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(
            db,
            BuiltinTypeKind::Int { width: IntWidth::I32, signed: false },
        )
        .into()
    }

    pub fn u64(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(
            db,
            BuiltinTypeKind::Int { width: IntWidth::I64, signed: false },
        )
        .into()
    }

    pub fn u128(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(
            db,
            BuiltinTypeKind::Int { width: IntWidth::I64, signed: false },
        )
        .into()
    }

    pub fn int(db: &dyn Db) -> Self {
        Self::i32(db)
    }

    pub fn usize(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(
            db,
            BuiltinTypeKind::Int { width: db.target_width(), signed: false },
        )
        .into()
    }

    pub fn isize(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(
            db,
            BuiltinTypeKind::Int { width: db.target_width(), signed: true },
        )
        .into()
    }

    pub fn bool(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(db, BuiltinTypeKind::Bool).into()
    }

    pub fn char(db: &dyn Db) -> Self {
        Self::u8(db)
    }

    pub fn void(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(db, BuiltinTypeKind::Void).into()
    }

    pub fn never(db: &dyn Db) -> Self {
        BuiltinTypeDef::new(db, BuiltinTypeKind::Never).into()
    }

    pub fn kind(&self, db: &dyn Db) -> BuiltinTypeKind {
        *self.interned().kind(db)
    }

    pub fn template_count(&self, db: &dyn Db) -> usize {
        match self.kind(db) {
            BuiltinTypeKind::Void
            | BuiltinTypeKind::Never
            | BuiltinTypeKind::Bool
            | BuiltinTypeKind::Int { .. } => 0,
            BuiltinTypeKind::Ref { .. }
            | BuiltinTypeKind::Ptr { .. }
            | BuiltinTypeKind::Slice => 1,
            BuiltinTypeKind::Tuple => 0,
        }
    }

    pub fn is_ptr_like(self, db: &dyn Db) -> Option<PtrKind> {
        match self.kind(db) {
            BuiltinTypeKind::Ref { mutability } => Some(PtrKind::Ref(mutability)),
            BuiltinTypeKind::Ptr { mutability } => Some(PtrKind::RawPtr(mutability)),
            _ => None,
        }
    }

    pub fn is_int_like(self, db: &dyn Db) -> Option<Self> {
        match self.kind(db) {
            BuiltinTypeKind::Bool
            | BuiltinTypeKind::Int { .. }
            | BuiltinTypeKind::Ptr { .. } => Some(self),
            _ => None,
        }
    }
}

impl TypeDefId {
    pub fn is_ptr_like(self, db: &dyn Db) -> Option<PtrKind> {
        match self {
            TypeDefId::Builtin(ty) => ty.is_ptr_like(db),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PtrKind {
    Ref(Mutability),
    RawPtr(Mutability),
}

impl PtrKind {
    pub fn mutability(&self) -> Mutability {
        match self {
            PtrKind::Ref(m) | PtrKind::RawPtr(m) => *m,
        }
    }
}

pub fn ptr_of(db: &dyn Db, ty: TypeRef, mutable: bool) -> TypeId {
    if mutable { mut_ptr_of(db, ty) } else { const_ptr_of(db, ty) }
}

pub fn const_ptr_of(db: &dyn Db, ty: TypeRef) -> TypeId {
    TypeId::new(db, BuiltinTypeId::const_ptr(db).into(), vec![ty])
}

pub fn mut_ptr_of(db: &dyn Db, ty: TypeRef) -> TypeId {
    TypeId::new(db, BuiltinTypeId::mut_ptr(db).into(), vec![ty])
}

pub fn const_ref_of(db: &dyn Db, ty: TypeRef) -> TypeId {
    TypeId::new(db, BuiltinTypeId::const_ref(db).into(), vec![ty])
}

pub fn mut_ref_of(db: &dyn Db, ty: TypeRef) -> TypeId {
    TypeId::new(db, BuiltinTypeId::mut_ref(db).into(), vec![ty])
}

pub fn ref_of(db: &dyn Db, ty: TypeRef, mutable: bool) -> TypeId {
    if mutable { mut_ref_of(db, ty) } else { const_ref_of(db, ty) }
}

pub fn slice_of(db: &dyn Db, ty: TypeRef) -> TypeId {
    TypeId::new(db, BuiltinTypeId::slice(db).into(), vec![ty])
}

pub fn tuple_of(db: &dyn Db, tys: Vec<TypeRef>) -> TypeId {
    TypeId::new(db, BuiltinTypeId::tuple(db).into(), tys)
}

pub fn int_id(db: &dyn Db) -> TypeId {
    TypeId::new(db, BuiltinTypeId::int(db).into(), vec![])
}

pub fn usize_id(db: &dyn Db) -> TypeId {
    TypeId::new(db, BuiltinTypeId::usize(db).into(), vec![])
}

pub fn char_id(db: &dyn Db) -> TypeId {
    TypeId::new(db, BuiltinTypeId::char(db).into(), vec![])
}

#[salsa::tracked(returns(copy))]
pub fn str_def(db: &dyn Db) -> TypeDefId {
    // in std::io
    let Definition::Type(def) = resolve_path(
        db,
        Segments::new(
            db,
            nonempty![
                Symbol::new(db, "core"),
                Symbol::new(db, "io"),
                Symbol::new(db, "str")
            ],
        ),
        core_module(db),
    )
    .unwrap() else {
        panic!()
    };
    def
}

#[salsa::tracked(returns(copy))]
pub fn str_id(db: &dyn Db) -> TypeId {
    TypeId::new(db, str_def(db), vec![])
}

#[salsa::tracked(returns(copy))]
pub fn void_id(db: &dyn Db) -> TypeId {
    TypeId::new(db, BuiltinTypeId::void(db).into(), vec![])
}

#[salsa::tracked(returns(copy))]
pub fn bool_id(db: &dyn Db) -> TypeId {
    TypeId::new(db, BuiltinTypeId::bool(db).into(), vec![])
}

#[salsa::tracked(returns(copy))]
pub fn never_id(db: &dyn Db) -> TypeId {
    TypeId::new(db, BuiltinTypeId::never(db).into(), vec![])
}

impl<'db> From<InternedInterfaceRef<'db>> for InterfaceRef {
    fn from(v: InternedInterfaceRef<'db>) -> Self {
        Self(v.0)
    }
}

impl<'db> From<InterfaceRef> for InternedInterfaceRef<'db> {
    fn from(v: InterfaceRef) -> Self {
        Self(v.0, PhantomData)
    }
}

impl InterfaceRef {
    pub fn new(db: &dyn Db, def: InterfaceId, args: Vec<TypeRef>) -> Self {
        InternedInterfaceRef::new(db, def, args).into()
    }

    pub fn interned(self) -> InternedInterfaceRef<'static> {
        InternedInterfaceRef(self.0, PhantomData)
    }

    pub fn def(self, db: &dyn Db) -> InterfaceId {
        *self.interned().def(db)
    }

    pub fn args(self, db: &dyn Db) -> &[TypeRef] {
        self.interned().args(db)
    }
}

impl<'db> From<InternedEnumId<'db>> for EnumId {
    fn from(v: InternedEnumId<'db>) -> Self {
        EnumId(v.0)
    }
}

impl<'db> From<EnumId> for InternedEnumId<'db> {
    fn from(v: EnumId) -> Self {
        Self(v.0, PhantomData)
    }
}

impl EnumId {
    pub fn new(db: &dyn Db, name: Symbol, parent: ModuleId) -> Self {
        InternedEnumId::new(db, name, parent).into()
    }

    pub fn interned(self) -> InternedEnumId<'static> {
        InternedEnumId(self.0, PhantomData)
    }

    pub fn name(self, db: &dyn Db) -> Symbol {
        *self.interned().name(db)
    }

    pub fn parent(self, db: &dyn Db) -> ModuleId {
        *self.interned().parent(db)
    }
}
