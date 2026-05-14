use std::collections::HashMap;
use std::sync::Arc;

use nonempty::nonzero::NonEmpty;

use crate::Db;

use crate::common::location::Span;
use crate::common::symbols::{InternedSymbol, Symbol};
use crate::name_resolve::{builtin_module, module_items};
use crate::parse_tree::top_level::{AstTemplateArg, AstTopLevelItem, AstTopLevelItemDesc};
use crate::parse_tree::type_expr::{
    AstAnyTypeExpr, AstAnyTypeExprDesc, AstTypeExpr, AstTypeExprDesc,
};
use crate::ril::{
    FileModule, FunctionId, InterfaceId, InternedModuleId, ModuleId, Package, ScopeOwnerId,
    StructId, TypeDefId, TypeId, TypeParamId, TypeRef, char_id, int_id, ptr_of, slice_of, str_id,
    void_id,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Definition {
    Function(FunctionId),
    Interface(InterfaceId),
    Module(ModuleId),
    Type(TypeDefId), // ... TODO
}

impl TypeDefId {
    pub fn name(&self, db: &dyn crate::Db) -> Symbol {
        match self {
            TypeDefId::Builtin(builtin) => builtin.name(db),
            TypeDefId::Struct(struct_id) => struct_id.name(db),
            TypeDefId::Interface(interface_id) => interface_id.name(db),
        }
    }
}

impl Definition {
    pub fn name(&self, db: &dyn Db) -> Symbol {
        match self {
            Definition::Function(func) => func.name(db),
            Definition::Type(def) => def.name(db),
            Definition::Interface(iface) => iface.name(db),
            Definition::Module(module) => module.name(db),
            // ... TODO
        }
    }
}

fn definition_of_item<'db>(
    db: &'db dyn Db,
    parent: InternedModuleId<'db>,
    item: &'db AstTopLevelItem,
) -> Option<Definition> {
    let m_id = ModuleId::from(parent);
    match &item.data {
        AstTopLevelItemDesc::Module(module) => Some(Definition::Module(ModuleId::new(
            db,
            module.data.name,
            Some(m_id),
            None,
            vec![],
            parent.package(db),
        ))),
        AstTopLevelItemDesc::Fundef(fundef) => Some(Definition::Function(FunctionId::new(
            db,
            fundef.data.name,
            ScopeOwnerId::Module(m_id),
        ))),
        AstTopLevelItemDesc::Interface(interface) => Some(Definition::Interface(InterfaceId::new(
            db,
            interface.name,
            m_id,
        ))),
        AstTopLevelItemDesc::Impl(_) => None,
        AstTopLevelItemDesc::StructDef(struct_def) => Some(Definition::Type(TypeDefId::Struct(
            StructId::new(db, struct_def.name, m_id.into()),
        ))),
        AstTopLevelItemDesc::Const(_) => todo!(
            "Not handled yet for multiple reasons: Not handled in parsing and need to unfold pattern definitions"
        ),
    }
}

#[salsa::tracked]
pub fn builtin_definitions<'db>(db: &'db dyn Db) -> HashMap<Symbol, Definition> {
    HashMap::from([
        (Symbol::new(db, "int"), Definition::Type(int_id(db).def(db))),
        (
            Symbol::new(db, "void"),
            Definition::Type(void_id(db).def(db)),
        ),
        (
            Symbol::new(db, "char"),
            Definition::Type(char_id(db).def(db)),
        ),
        (Symbol::new(db, "str"), Definition::Type(str_id(db).def(db))),
    ])
}

#[salsa::tracked]
pub fn file_module_id<'db>(
    db: &'db dyn Db,
    fm: FileModule<'db>,
    parent: Option<ModuleId>,
    package: Package<'db>,
) -> ModuleId {
    ModuleId::new(
        db,
        fm.name(db),
        parent,
        Some(fm.file(db).to_owned(db)),
        fm.submodules(db).clone(),
        package,
    )
}

#[salsa::tracked]
pub fn file_module_definitions<'db>(
    db: &'db dyn Db,
    file_module: FileModule<'db>,
    parent: Option<ModuleId>,
    package: Package<'db>,
) -> HashMap<Symbol, Definition> {
    let module_id = file_module_id(db, file_module, parent, package);
    let mut defs = HashMap::new();
    for sub in file_module.submodules(db) {
        let sub_id = file_module_id(db, *sub, Some(module_id), package);
        defs.insert(sub.name(db), Definition::Module(sub_id));
    }
    defs.extend(module_definitions(db, module_id.interned()));
    defs
}

#[salsa::tracked]
pub fn module_definitions<'db>(
    db: &'db dyn Db,
    module: InternedModuleId<'db>,
) -> HashMap<Symbol, Definition> {
    if module == builtin_module(db, module.package(db)).interned() {
        builtin_definitions(db)
    } else {
        let mut res = HashMap::new();
        for sub in module.file_submodules(db) {
            let id = file_module_id(db, sub, Some(module.into()), module.package(db));
            res.insert(id.name(db), Definition::Module(id));
        }
        let items = module_items(db, module);
        items.iter().for_each(|items| {
            items.iter().for_each(|item| {
                definition_of_item(db, module, item)
                    .into_iter()
                    .for_each(|def| {
                        res.insert(def.name(db), def);
                    })
            })
        });

        res
    }
}

#[salsa::tracked]
pub fn resolve_in_module<'db>(
    db: &'db dyn Db,
    name: InternedSymbol<'db>,
    module: InternedModuleId<'db>,
) -> Option<Definition> {
    let mut defs = module_definitions(db, module);
    defs.remove(&Symbol::from(name)).or_else(|| {
        module
            .parent(db)
            .map(|parent| resolve_in_module(db, name, parent.interned()))
            .flatten()
    })
}

#[salsa::tracked]
pub struct Segments<'db> {
    pub segments: NonEmpty<Symbol>,
}

#[salsa::tracked]
pub fn resolve_path<'db>(
    db: &'db dyn Db,
    segments: Segments<'db>,
    module: InternedModuleId<'db>,
) -> Option<Definition> {
    let segments = segments.segments(db);
    let (head, tail) = segments.split_first();
    tail.iter().fold(
        resolve_in_module(db, head.interned(), module),
        |acc, name| {
            if let Some(Definition::Module(module_id)) = acc {
                resolve_in_module(db, name.interned(), module_id.interned())
            } else {
                None
            }
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeResolution {
    Error,
    Infer,
    Type(TypeRef),
}

pub fn resolve_any_type_expr<'db>(
    db: &'db dyn Db,
    any_type_expr: &'db AstAnyTypeExpr,
    module: InternedModuleId<'db>,
    template_args: &'db [AstTemplateArg], // From the enclosing item
) -> TypeResolution {
    match &any_type_expr.data {
        AstAnyTypeExprDesc::Any => TypeResolution::Infer,
        AstAnyTypeExprDesc::Known(desc) => {
            resolve_spanned_type_expr_desc(db, desc, &any_type_expr.span, module, template_args)
        }
    }
}

pub fn resolve_spanned_type_expr_desc<'db>(
    db: &'db dyn Db,
    type_expr: &'db AstTypeExprDesc,
    _span: &'db Span,
    module: InternedModuleId<'db>,
    template_args: &'db [AstTemplateArg], // From the enclosing item
) -> TypeResolution {
    match type_expr {
        AstTypeExprDesc::Named { name, args } => {
            if args.is_empty()
                && let Some(idx) = template_args.iter().position(|p| p.name == *name)
            {
                return TypeResolution::Type(TypeRef::Param(TypeParamId(idx)));
            }

            resolve_in_module(db, name.interned(), module).map_or(TypeResolution::Error, |def| {
                if let Definition::Type(type_def_id) = def {
                    let resolved_args = args
                        .iter()
                        .map(
                            |arg| match resolve_any_type_expr(db, arg, module, template_args) {
                                TypeResolution::Type(type_ref) => Some(type_ref),
                                _ => None,
                            },
                        )
                        .collect::<Option<Vec<_>>>();
                    match resolved_args {
                        Some(resolved_args) => {
                            TypeResolution::Type(TypeId::new(db, type_def_id, resolved_args).into())
                        }
                        None => TypeResolution::Error,
                    }
                } else {
                    TypeResolution::Error
                }
            })
        }
        AstTypeExprDesc::NameResolved { from, to } => {
            if let Some(Definition::Module(module)) = resolve_in_module(db, from.interned(), module)
            {
                resolve_type_expr(db, &**to, module.interned(), template_args)
            } else {
                TypeResolution::Error
            }
        }
        AstTypeExprDesc::Pointer(pointee) => {
            match resolve_type_expr(db, &*pointee, module, template_args) {
                TypeResolution::Type(pointee) => TypeResolution::Type(ptr_of(db, pointee).into()),
                _ => TypeResolution::Error,
            }
        }
        AstTypeExprDesc::Slice { ty, len } => {
            assert!(len.is_none(), "TODO: handle non value-type");
            match resolve_type_expr(db, &*ty, module, template_args) {
                TypeResolution::Type(elem) => TypeResolution::Type(slice_of(db, elem).into()),
                _ => TypeResolution::Infer,
            }
        }
    }
}

#[inline(always)]
pub fn resolve_type_expr<'db>(
    db: &'db dyn Db,
    type_expr: &'db AstTypeExpr,
    module: InternedModuleId<'db>,
    template_args: &'db [AstTemplateArg], // From the enclosing item
) -> TypeResolution {
    resolve_spanned_type_expr_desc(db, &type_expr.data, &type_expr.span, module, template_args)
}

#[salsa::tracked]
pub fn get_module_pretty_name<'db>(db: &'db dyn Db, id: InternedModuleId<'db>) -> Arc<String> {
    let prefix = if let Some(parent) = id.parent(db) {
        format!("{}::", get_module_pretty_name(db, parent.interned()))
    } else {
        format!("")
    };
    Arc::new(format!("{}{}", prefix, id.name(db).interned().contents(db)))
}
