// Rockr Intermediate Language
#![allow(dead_code)]

use std::marker::PhantomData;

use crate::{OwnedSourceFile, common::symbols::Symbol};

#[salsa::interned]
pub struct InternedModuleId {
    pub name: Symbol,
    pub parent: Option<ModuleId>,
    pub file: Option<OwnedSourceFile>,
}

#[salsa::interned]
pub struct InternedFunctionId {
    pub name: Symbol,
    pub parent: ScopeOwnerId,
}

#[salsa::interned]
pub struct InternedStructId {
    pub name: Symbol,
    pub parent: ModuleId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeParamId(usize);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeParam {
    pub name: Symbol,
    pub constraints: Vec<InterfaceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeRef {
    Concrete(TypeId),
    Param(TypeParamId),
}

#[salsa::interned]
pub struct InternedImplId {
    pub parent: ModuleId,
    pub implemented: TypeRef,
    pub interface: Option<InterfaceId>,
}

#[salsa::interned]
pub struct InternedInterfaceId {
    pub name: Symbol,
    pub parent: ModuleId,
}

#[salsa::interned]
pub struct InternedTypeId {
    pub def: TypeDefId,
    pub args: Vec<TypeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeOwnerId {
    Module(ModuleId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeDefId {
    Struct(StructId),
    Interface(InterfaceId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImplId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InterfaceId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(salsa::Id);

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
    pub fn new(
        db: &dyn crate::Db,
        name: Symbol,
        parent: Option<ModuleId>,
        file: Option<OwnedSourceFile>,
    ) -> Self {
        InternedModuleId::new(db, name, parent, file).into()
    }

    pub fn interned(self) -> InternedModuleId<'static> {
        InternedModuleId(self.0, PhantomData)
    }

    pub fn name(self, db: &dyn crate::Db) -> Symbol {
        self.interned().name(db)
    }

    pub fn parent(self, db: &dyn crate::Db) -> Option<ModuleId> {
        self.interned().parent(db)
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

impl FunctionId {
    pub fn new(db: &dyn crate::Db, name: Symbol, parent: ScopeOwnerId) -> Self {
        InternedFunctionId::new(db, name, parent).into()
    }

    pub fn interned(self) -> InternedFunctionId<'static> {
        InternedFunctionId(self.0, PhantomData)
    }

    pub fn name(self, db: &dyn crate::Db) -> Symbol {
        self.interned().name(db)
    }

    pub fn parent(self, db: &dyn crate::Db) -> ScopeOwnerId {
        self.interned().parent(db)
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
        self.interned().name(db)
    }

    pub fn parent(self, db: &dyn crate::Db) -> ModuleId {
        self.interned().parent(db)
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

impl ImplId {
    pub fn new(
        db: &dyn crate::Db,
        parent: ModuleId,
        implemented: TypeRef,
        interface: Option<InterfaceId>,
    ) -> Self {
        InternedImplId::new(db, parent, implemented, interface).into()
    }

    pub fn interned(self) -> InternedImplId<'static> {
        InternedImplId(self.0, PhantomData)
    }

    pub fn parent(self, db: &dyn crate::Db) -> ModuleId {
        self.interned().parent(db)
    }

    pub fn implemented(self, db: &dyn crate::Db) -> TypeRef {
        self.interned().implemented(db)
    }

    pub fn interface(self, db: &dyn crate::Db) -> Option<InterfaceId> {
        self.interned().interface(db)
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
        self.interned().name(db)
    }

    pub fn parent(self, db: &dyn crate::Db) -> ModuleId {
        self.interned().parent(db)
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
    pub fn new(db: &dyn crate::Db, def: TypeDefId, args: Vec<TypeId>) -> Self {
        InternedTypeId::new(db, def, args).into()
    }

    pub fn interned(self) -> InternedTypeId<'static> {
        InternedTypeId(self.0, PhantomData)
    }

    pub fn def(self, db: &dyn crate::Db) -> TypeDefId {
        self.interned().def(db)
    }

    pub fn args(self, db: &dyn crate::Db) -> Vec<TypeId> {
        self.interned().args(db)
    }
}
