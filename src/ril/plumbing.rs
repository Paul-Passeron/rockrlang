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
        db: &'db dyn crate::Db,
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

    pub fn name(self, db: &dyn crate::Db) -> Symbol {
        *self.interned().name(db)
    }

    pub fn parent(self, db: &dyn crate::Db) -> Option<ModuleId> {
        *self.interned().parent(db)
    }

    pub fn file_submodules(self, db: &dyn crate::Db) -> &[FileModule<'_>] {
        self.interned().file_submodules(db)
    }

    pub fn package(self, db: &dyn crate::Db) -> Option<Package<'_>> {
        *self.interned().package(db)
    }

    pub fn owning_package(self, db: &dyn crate::Db) -> Option<Package<'_>> {
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
    pub fn new(db: &dyn crate::Db, name: Symbol, parent: ScopeOwnerId) -> Self {
        InternedFunctionId::new(db, name, parent).into()
    }

    pub fn interned(self) -> InternedFunctionId<'static> {
        InternedFunctionId(self.0, PhantomData)
    }

    pub fn name(self, db: &dyn crate::Db) -> Symbol {
        *self.interned().name(db)
    }

    pub fn parent(self, db: &dyn crate::Db) -> ScopeOwnerId {
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
    pub fn new(db: &dyn crate::Db, name: Symbol, parent: ModuleId) -> Self {
        InternedStructId::new(db, name, parent).into()
    }

    pub fn interned(self) -> InternedStructId<'static> {
        InternedStructId(self.0, PhantomData)
    }

    pub fn name(self, db: &dyn crate::Db) -> Symbol {
        *self.interned().name(db)
    }

    pub fn parent(self, db: &dyn crate::Db) -> ModuleId {
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
        db: &dyn crate::Db,
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

    pub fn parent(self, db: &dyn crate::Db) -> ModuleId {
        *self.interned().parent(db)
    }

    pub fn implemented(self, db: &dyn crate::Db) -> TypeRef {
        *self.interned().implemented(db)
    }

    pub fn interface(self, db: &dyn crate::Db) -> Option<InterfaceRef> {
        *self.interned().interface(db)
    }

    pub fn templates(self, db: &dyn crate::Db) -> &[Set<InterfaceRef>] {
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
    pub fn new(db: &dyn crate::Db, name: Symbol, parent: ModuleId) -> Self {
        InternedInterfaceId::new(db, name, parent).into()
    }

    pub fn interned(self) -> InternedInterfaceId<'static> {
        InternedInterfaceId(self.0, PhantomData)
    }

    pub fn name(self, db: &dyn crate::Db) -> Symbol {
        *self.interned().name(db)
    }

    pub fn parent(self, db: &dyn crate::Db) -> ModuleId {
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
    pub fn new(db: &dyn crate::Db, def: TypeDefId, args: Vec<TypeRef>) -> Self {
        InternedTypeId::new(db, def, args).into()
    }

    pub fn interned(self) -> InternedTypeId<'static> {
        InternedTypeId(self.0, PhantomData)
    }

    pub fn def(self, db: &dyn crate::Db) -> TypeDefId {
        *self.interned().def(db)
    }

    pub fn args(self, db: &dyn crate::Db) -> &[TypeRef] {
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

impl BuiltinTypeId {
    pub fn new(db: &dyn crate::Db, name: Symbol) -> Self {
        Self(InternedBuiltinTypeId::new(db, name).0)
    }

    pub fn interned(self) -> InternedBuiltinTypeId<'static> {
        InternedBuiltinTypeId(self.0, PhantomData)
    }

    pub fn name(self, db: &dyn crate::Db) -> Symbol {
        *self.interned().name(db)
    }

    pub fn ptr(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "*"))
    }
    pub fn mut_ptr(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "*mut"))
    }

    pub fn ref_(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "&"))
    }
    pub fn mut_ref(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "&mut"))
    }

    pub fn tuple(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "()"))
    }

    pub fn slice(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "[]"))
    }

    pub fn int(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "int"))
    }

    pub fn usize(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "usize"))
    }

    pub fn bool(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "bool"))
    }

    pub fn char(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "char"))
    }

    pub fn void(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "void"))
    }

    pub fn never(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "never"))
    }

    pub fn template_count(&self, db: &dyn crate::Db) -> usize {
        match self.name(db).interned().contents(db).as_str() {
            "*mut" | "*" | "&" | "&mut" | "[]" => 1,
            _ => 0,
        }
    }

    pub fn is_ptr_like(self, db: &dyn crate::Db) -> Option<PtrKind> {
        match self.name(db).interned().contents(db).as_str() {
            "*mut" => Some(PtrKind::RawPtr(Mutability::Mutable)),
            "*" => Some(PtrKind::RawPtr(Mutability::Const)),
            "&mut" => Some(PtrKind::Ref(Mutability::Mutable)),
            "&" => Some(PtrKind::Ref(Mutability::Const)),
            _ => None,
        }
    }

    pub fn is_int_like(self, db: &dyn Db) -> Option<Self> {
        (self == BuiltinTypeId::int(db)
            || self == BuiltinTypeId::usize(db)
            || self == BuiltinTypeId::char(db)
            || self == BuiltinTypeId::mut_ptr(db)
            || self == BuiltinTypeId::ptr(db))
        .then_some(self)
    }
}

impl TypeDefId {
    pub fn is_ptr_like(self, db: &dyn crate::Db) -> Option<PtrKind> {
        match self {
            TypeDefId::Builtin(ty) => ty.is_ptr_like(db),
            _ => None,
        }
    }
}

pub enum PtrKind {
    Ref(Mutability),
    RawPtr(Mutability),
}

pub fn ptr_of(db: &dyn crate::Db, ty: TypeRef, mutable: bool) -> TypeId {
    if mutable { mut_ptr_of(db, ty) } else { const_ptr_of(db, ty) }
}

pub fn const_ptr_of(db: &dyn crate::Db, ty: TypeRef) -> TypeId {
    TypeId::new(db, BuiltinTypeId::ptr(db).into(), vec![ty])
}

pub fn mut_ptr_of(db: &dyn crate::Db, ty: TypeRef) -> TypeId {
    TypeId::new(db, BuiltinTypeId::mut_ptr(db).into(), vec![ty])
}

pub fn const_ref_of(db: &dyn crate::Db, ty: TypeRef) -> TypeId {
    TypeId::new(db, BuiltinTypeId::ref_(db).into(), vec![ty])
}

pub fn mut_ref_of(db: &dyn crate::Db, ty: TypeRef) -> TypeId {
    TypeId::new(db, BuiltinTypeId::mut_ref(db).into(), vec![ty])
}

pub fn ref_of(db: &dyn crate::Db, ty: TypeRef, mutable: bool) -> TypeId {
    if mutable { mut_ref_of(db, ty) } else { const_ref_of(db, ty) }
}

pub fn slice_of(db: &dyn crate::Db, ty: TypeRef) -> TypeId {
    TypeId::new(db, BuiltinTypeId::slice(db).into(), vec![ty])
}

pub fn tuple_of(db: &dyn crate::Db, tys: Vec<TypeRef>) -> TypeId {
    TypeId::new(db, BuiltinTypeId::tuple(db).into(), tys)
}

pub fn int_id(db: &dyn crate::Db) -> TypeId {
    TypeId::new(db, BuiltinTypeId::int(db).into(), vec![])
}

pub fn usize_id(db: &dyn crate::Db) -> TypeId {
    TypeId::new(db, BuiltinTypeId::usize(db).into(), vec![])
}

pub fn char_id(db: &dyn crate::Db) -> TypeId {
    TypeId::new(db, BuiltinTypeId::char(db).into(), vec![])
}

#[salsa::tracked(returns(copy))]
pub fn str_def(db: &dyn crate::Db) -> TypeDefId {
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
pub fn str_id(db: &dyn crate::Db) -> TypeId {
    TypeId::new(db, str_def(db), vec![])
}

#[salsa::tracked(returns(copy))]
pub fn void_id(db: &dyn crate::Db) -> TypeId {
    TypeId::new(db, BuiltinTypeId::void(db).into(), vec![])
}

#[salsa::tracked(returns(copy))]
pub fn bool_id(db: &dyn crate::Db) -> TypeId {
    TypeId::new(db, BuiltinTypeId::bool(db).into(), vec![])
}

#[salsa::tracked(returns(copy))]
pub fn never_id(db: &dyn crate::Db) -> TypeId {
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
    pub fn new(db: &dyn crate::Db, def: InterfaceId, args: Vec<TypeRef>) -> Self {
        InternedInterfaceRef::new(db, def, args).into()
    }

    pub fn interned(self) -> InternedInterfaceRef<'static> {
        InternedInterfaceRef(self.0, PhantomData)
    }

    pub fn def(self, db: &dyn crate::Db) -> InterfaceId {
        *self.interned().def(db)
    }

    pub fn args(self, db: &dyn crate::Db) -> &[TypeRef] {
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
    pub fn new(db: &dyn crate::Db, name: Symbol, parent: ModuleId) -> Self {
        InternedEnumId::new(db, name, parent).into()
    }

    pub fn interned(self) -> InternedEnumId<'static> {
        InternedEnumId(self.0, PhantomData)
    }

    pub fn name(self, db: &dyn crate::Db) -> Symbol {
        *self.interned().name(db)
    }

    pub fn parent(self, db: &dyn crate::Db) -> ModuleId {
        *self.interned().parent(db)
    }
}
