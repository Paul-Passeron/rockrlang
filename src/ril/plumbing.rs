use std::marker::PhantomData;

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
        file: Option<OwnedSourceFile>,
        file_submodules: Vec<FileModule<'db>>,
        package: Option<Package<'db>>,
    ) -> Self {
        InternedModuleId::new(db, name, parent, file, file_submodules, package).into()
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

    pub fn file_submodules(self, db: &dyn crate::Db) -> Vec<FileModule<'_>> {
        self.interned().file_submodules(db)
    }

    pub fn package(self, db: &dyn crate::Db) -> Option<Package<'_>> {
        self.interned().package(db)
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

#[allow(dead_code)]
impl ImplId {
    pub fn new<'db>(
        db: &'db dyn crate::Db,
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
        self.interned().parent(db)
    }

    pub fn implemented(self, db: &dyn crate::Db) -> TypeRef {
        self.interned().implemented(db)
    }

    pub fn interface(self, db: &dyn crate::Db) -> Option<InterfaceRef> {
        self.interned().interface(db)
    }

    pub fn templates(self, db: &dyn crate::Db) -> Vec<Set<InterfaceRef>> {
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
    pub fn new(db: &dyn crate::Db, def: TypeDefId, args: Vec<TypeRef>) -> Self {
        InternedTypeId::new(db, def, args).into()
    }

    pub fn interned(self) -> InternedTypeId<'static> {
        InternedTypeId(self.0, PhantomData)
    }

    pub fn def(self, db: &dyn crate::Db) -> TypeDefId {
        self.interned().def(db)
    }

    pub fn args(self, db: &dyn crate::Db) -> Vec<TypeRef> {
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
        self.interned().name(db)
    }

    pub fn ptr(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "*"))
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

    pub fn char(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "char"))
    }

    pub fn str(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "str"))
    }

    pub fn void(db: &dyn crate::Db) -> Self {
        Self::new(db, Symbol::new(db, "void"))
    }
}

pub fn ptr_of(db: &dyn crate::Db, ty: TypeRef) -> TypeId {
    TypeId::new(db, BuiltinTypeId::ptr(db).into(), vec![ty])
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

pub fn char_id(db: &dyn crate::Db) -> TypeId {
    TypeId::new(db, BuiltinTypeId::char(db).into(), vec![])
}

pub fn str_id(db: &dyn crate::Db) -> TypeId {
    TypeId::new(db, BuiltinTypeId::str(db).into(), vec![])
}

pub fn void_id(db: &dyn crate::Db) -> TypeId {
    TypeId::new(db, BuiltinTypeId::void(db).into(), vec![])
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
        self.interned().def(db)
    }

    pub fn args(self, db: &dyn crate::Db) -> Vec<TypeRef> {
        self.interned().args(db)
    }
}
