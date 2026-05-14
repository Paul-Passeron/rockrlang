// Rockr Intermediate Language

pub mod display;
pub mod plumbing;

pub use plumbing::*;

use crate::{
    Db, OwnedSourceFile, SourceFile,
    common::{location::Span, symbols::Symbol, unord::Set},
    name_resolve::type_expr::{templates_of_enum, templates_of_struct},
    parse_tree::top_level::{AstImplItem, AstTemplateArg},
};

#[salsa::tracked]
pub struct FileModule<'db> {
    pub file: SourceFile<'db>,
    #[returns(ref)]
    pub submodules: Vec<FileModule<'db>>,
}

impl<'db> FileModule<'db> {
    pub fn name(&self, db: &'db dyn crate::Db) -> Symbol {
        let path = self.file(db).path(db);
        let stem = path
            .parent()
            .and_then(|parent| {
                // If this file is main.rkr, the module name is the directory name
                if path.file_name().and_then(|n| n.to_str()) == Some("main.rkr") {
                    parent.file_name()
                } else {
                    None
                }
            })
            .or_else(|| path.file_stem())
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        Symbol::new(db, stem)
    }
}

#[salsa::tracked]
pub struct Package<'db> {
    pub root: FileModule<'db>,
}

#[salsa::interned]
pub struct InternedModuleId {
    pub name: Symbol,
    pub parent: Option<ModuleId>,
    pub file: Option<OwnedSourceFile>,
    pub file_submodules: Vec<FileModule<'db>>,
    pub package: Option<Package<'db>>,
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

#[salsa::interned]
pub struct InternedEnumId {
    pub name: Symbol,
    pub parent: ModuleId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeParamId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub struct TypeParam {
    pub name: Symbol,
    pub constraints: Vec<InterfaceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeRef {
    Concrete(TypeId),
    Param(TypeParamId),
    Associated(Symbol),
    Zelf,
    Error,
}

#[salsa::interned]
pub struct InternedImplId {
    pub parent: ModuleId,
    pub implemented: TypeRef,
    pub interface: Option<InterfaceRef>,
    pub templates: Vec<Set<InterfaceRef>>,
}

#[salsa::interned]
pub struct InternedInterfaceId {
    pub name: Symbol,
    pub parent: ModuleId,
}

#[salsa::interned]
pub struct InternedInterfaceRef {
    pub def: InterfaceId,
    pub args: Vec<TypeRef>,
}

#[salsa::interned]
pub struct InternedTypeId {
    pub def: TypeDefId,
    pub args: Vec<TypeRef>, // None means inferred
}

#[salsa::interned]
pub struct InternedBuiltinTypeId {
    pub name: Symbol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeOwnerId {
    Module(ModuleId),
    Impl(ImplId),
    Interface(InterfaceRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeDefId {
    Builtin(BuiltinTypeId),
    Struct(StructId),
    Enum(EnumId),
}

#[salsa::tracked]
#[derive(Debug, PartialOrd, Ord)]
pub struct ImplSource<'db> {
    pub id: ImplId,
    pub module: ModuleId,
    pub templates: Vec<AstTemplateArg>,
    pub items: Vec<AstImplItem>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FunctionId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StructId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnumId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImplId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InterfaceId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuiltinTypeId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InterfaceRef(salsa::Id);

pub fn get_template_param_count(db: &dyn Db, ty: TypeDefId) -> usize {
    match ty {
        TypeDefId::Builtin(builtin_type_id) => builtin_type_id.template_count(db),
        TypeDefId::Struct(struct_id) => templates_of_struct(db, struct_id.interned()).len(),
        TypeDefId::Enum(enum_id) => templates_of_enum(db, enum_id.interned()).len(),
    }
}

impl ModuleId {
    pub fn get_span(&self, db: &dyn Db) -> Span {
        todo!()
    }
}
