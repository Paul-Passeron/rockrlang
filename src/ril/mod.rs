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

// Rockr Intermediate Language

pub mod display;
pub mod plumbing;

pub use plumbing::*;

use crate::{
    Db, SourceFile,
    common::{location::Span, symbols::Symbol, unord::Set},
    hir::Mutability,
    layout::IntWidth,
    name_resolve::type_expr::{templates_of_enum, templates_of_struct},
    parse_tree::top_level::{AstImplItem, AstTemplateArg},
    printer::type_printer::TypePrinter,
};

#[salsa::tracked]
#[derive(PartialOrd, Ord)]
pub struct FileModule<'db> {
    pub file: SourceFile,
    #[returns(ref)]
    pub submodules: Vec<FileModule<'db>>,
}

impl<'db> FileModule<'db> {
    pub fn name(&self, db: &'db dyn crate::Db) -> Symbol {
        let path = self.file(db).path(db);
        let stem = path
            .parent()
            .and_then(|parent| {
                // If this file is main.rkr, the module name is the directory
                // name
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
#[derive(PartialOrd, Ord)]
pub struct Package<'db> {
    pub root: FileModule<'db>,
}

#[salsa::interned]
pub struct InternedModuleId<'db> {
    pub name: Symbol,
    pub parent: Option<ModuleId>,
    pub file: Option<SourceFile>,
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
    Unknown,
}

impl TypeRef {
    pub fn to_string(&self, db: &dyn Db) -> String {
        TypePrinter::new().type_ref_to_string(db, *self)
    }
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
pub struct BuiltinTypeDef {
    pub kind: BuiltinTypeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinTypeKind {
    Void,
    Never,

    Bool,
    Int { width: IntWidth, signed: bool },

    Ref { mutability: Mutability },
    Ptr { mutability: Mutability },

    Slice,
    Tuple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeOwnerId {
    Module(ModuleId),
    Impl(ImplId),
    Interface(InterfaceRef),
}

impl ScopeOwnerId {
    pub fn get_canonical_zelf(&self, db: &dyn Db) -> Option<TypeRef> {
        match self {
            ScopeOwnerId::Module(_) => None,
            ScopeOwnerId::Impl(impl_id) => Some(impl_id.implemented(db)),
            ScopeOwnerId::Interface(_) => Some(TypeRef::Zelf),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeDefId {
    Builtin(BuiltinTypeId),
    Struct(StructId),
    Enum(EnumId),
}

impl TypeDefId {
    pub fn to_string(&self, db: &dyn Db) -> String {
        TypePrinter::new().type_def_id_to_string(db, *self)
    }

    pub fn is_int_like(self, db: &dyn Db) -> Option<BuiltinTypeId> {
        match self {
            Self::Builtin(b) => b.is_int_like(db),
            _ => None,
        }
    }
}

#[salsa::tracked]
#[derive(Debug, PartialOrd, Ord)]
pub struct ImplSource<'db> {
    pub id: ImplId,
    pub module: ModuleId,
    #[returns(ref)]
    pub templates: Vec<AstTemplateArg>,
    #[returns(ref)]
    pub items: Vec<AstImplItem>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FunctionId(salsa::Id);

impl FunctionId {
    pub fn sig_to_string(self, db: &dyn Db) -> String {
        TypePrinter::new().function_id_to_string(db, self)
    }

    pub fn called_to_string(self, db: &dyn Db) -> String {
        TypePrinter::new().called_function_to_string(db, self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StructId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnumId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImplId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InterfaceId(salsa::Id);

impl InterfaceId {
    pub fn to_string(&self, db: &dyn Db) -> String {
        TypePrinter::new().interface_id_to_string(db, *self)
    }
}

impl InterfaceRef {
    pub fn to_string(&self, db: &dyn Db) -> String {
        TypePrinter::new().interface_ref_to_string(db, *self)
    }
}

impl ImplId {
    pub fn to_string(&self, db: &dyn Db) -> String {
        TypePrinter::new().impl_id_to_string(db, *self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuiltinTypeId(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InterfaceRef(salsa::Id);

pub fn templates_of_type_def(db: &dyn Db, ty: TypeDefId) -> usize {
    match ty {
        TypeDefId::Builtin(id) => id.template_count(db),
        TypeDefId::Struct(id) => templates_of_struct(db, id.into()).len(),
        TypeDefId::Enum(id) => templates_of_enum(db, id.into()).len(),
    }
}

pub fn merge_type_ref(db: &dyn Db, a: TypeRef, b: TypeRef) -> TypeRef {
    match (a, b) {
        (TypeRef::Param(a), TypeRef::Param(b)) => {
            if a != b {
                return TypeRef::Error;
            }
            TypeRef::Param(a)
        }
        (TypeRef::Param(a), _) | (_, TypeRef::Param(a)) => TypeRef::Param(a),
        (TypeRef::Zelf, _) | (_, TypeRef::Zelf) => TypeRef::Zelf,
        (TypeRef::Error, other) | (other, TypeRef::Error) => other,
        (TypeRef::Unknown, other) | (other, TypeRef::Unknown) => other,
        (TypeRef::Associated(s), _) | (_, TypeRef::Associated(s)) => {
            TypeRef::Associated(s)
        }
        (TypeRef::Concrete(a), TypeRef::Concrete(b)) => {
            let a_def = a.def(db);
            if a.def(db) != b.def(db) {
                return TypeRef::Error;
            }
            let a_args = a.args(db);
            let b_args = b.args(db);
            if a_args.len() != b_args.len() {
                return TypeRef::Error;
            }

            let args = a_args
                .iter()
                .zip(b_args)
                .map(|(a, b)| merge_type_ref(db, *a, *b))
                .collect();

            TypeId::new(db, a_def, args).into()
        }
    }
}

pub fn rehole(db: &dyn Db, ty: TypeRef) -> TypeRef {
    let Some(id) = ty.as_type_id() else {
        return ty;
    };
    let def = id.def(db);
    let n = templates_of_type_def(db, def);
    let args = id.args(db);
    let args = (0..n)
        .into_iter()
        .map(|i| args.get(i).copied().unwrap_or(TypeRef::Unknown))
        .collect();

    TypeId::new(db, def, args).into()
}

pub fn compute_template_hints(
    db: &dyn Db,
    ty: TypeRef,
    type_args: Vec<TypeRef>,
) -> Vec<TypeRef> {
    let Some(reholed) = rehole(db, ty).as_type_id() else {
        return type_args;
    };

    reholed
        .args(db)
        .iter()
        .zip(type_args)
        .map(|(a, b)| merge_type_ref(db, *a, b))
        .collect()
}
