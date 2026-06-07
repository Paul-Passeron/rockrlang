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

use crate::{
    Db,
    common::{
        location::Span,
        symbols::{InternedSymbol, Symbol},
    },
    hir::{FunctionLikeAst, function_ast},
    name_resolve::{
        builtin_module, core_module, core_package,
        interfaces::interface_item,
        module_items, std_module,
        type_expr::{enum_item, struct_item},
    },
    parse_tree::top_level::{AstIncludePathDesc, AstTopLevelItem, AstTopLevelItemDesc},
    parser::parse_file,
    printer::type_printer::TypePrinter,
    ril::{
        EnumId, FileModule, FunctionId, InterfaceId, InternedModuleId, ModuleId, Package,
        ScopeOwnerId, StructId, TypeDefId, bool_id, char_id, int_id, never_id, usize_id,
        void_id,
    },
};
use nonempty::NonEmpty;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Definition {
    Function(FunctionId),
    Interface(InterfaceId),
    Module(ModuleId),
    Type(TypeDefId), // ... TODO
}

impl Definition {
    pub fn to_string(self, db: &dyn Db) -> String {
        TypePrinter::new().definition_to_string(db, self)
    }

    // TODO: The current way we do things, we can't get the name span of other definitions of the same kind with the same name at top level.

    pub fn name(self, db: &dyn Db) -> Symbol {
        match self {
            Definition::Function(func) => func.name(db),
            Definition::Type(def) => def.name(db),
            Definition::Interface(iface) => iface.name(db),
            Definition::Module(module) => module.name(db),
        }
    }

    pub fn name_span(self, db: &dyn Db) -> Option<Span> {
        match self {
            Definition::Function(function_id) => Some(function_id.name_span(db)),
            Definition::Interface(interface_id) => Some(interface_id.name_span(db)),
            Definition::Module(module_id) => module_id.name_span(db),
            Definition::Type(type_def_id) => type_def_id.name_span(db),
        }
    }
}

impl ModuleId {
    pub fn name_span(self, db: &dyn Db) -> Option<Span> {
        let parent = self.parent(db)?;
        for item in module_items(db, parent.interned()).into_iter().flatten() {
            if let AstTopLevelItemDesc::Module(curr_mod) = &item.data
                && curr_mod.data.name.data == self.name(db)
            {
                return Some(curr_mod.data.name.span);
            }
        }
        unreachable!()
    }
}

impl TypeDefId {
    pub fn name(&self, db: &dyn crate::Db) -> Symbol {
        match self {
            TypeDefId::Builtin(builtin) => builtin.name(db),
            TypeDefId::Struct(struct_id) => struct_id.name(db),
            TypeDefId::Enum(enum_id) => enum_id.name(db),
        }
    }

    pub fn parent(&self, db: &dyn crate::Db) -> ModuleId {
        match self {
            TypeDefId::Builtin(_) => builtin_module(db),
            TypeDefId::Struct(struct_id) => struct_id.parent(db),
            TypeDefId::Enum(enum_id) => enum_id.parent(db),
        }
    }

    pub fn name_span(&self, db: &dyn Db) -> Option<Span> {
        match self {
            TypeDefId::Builtin(_) => None,
            TypeDefId::Struct(struct_id) => Some(struct_id.name_span(db)),
            TypeDefId::Enum(enum_id) => Some(enum_id.name_span(db)),
        }
    }

    pub fn span(&self, db: &dyn Db) -> Option<Span> {
        match self {
            TypeDefId::Builtin(_) => None,
            TypeDefId::Struct(struct_id) => Some(struct_id.span(db)),
            TypeDefId::Enum(enum_id) => Some(enum_id.span(db)),
        }
    }
}

impl StructId {
    pub fn name_span(&self, db: &dyn Db) -> Span {
        struct_item(db, self.interned()).name.span
    }

    pub fn span(&self, db: &dyn Db) -> Span {
        struct_item(db, self.interned()).span
    }
}

impl EnumId {
    pub fn name_span(&self, db: &dyn Db) -> Span {
        enum_item(db, self.interned()).name.span
    }

    pub fn span(&self, db: &dyn Db) -> Span {
        enum_item(db, self.interned()).span
    }
}

impl InterfaceId {
    pub fn name_span(&self, db: &dyn Db) -> Span {
        interface_item(db, self.interned()).name.span
    }
}

impl FunctionId {
    pub fn name_span(&self, db: &dyn Db) -> Span {
        function_ast(db, self.interned()).inner(db).name_span()
    }

    pub fn span(&self, db: &dyn Db) -> Span {
        function_ast(db, self.interned()).inner(db).get_span()
    }
}

impl FunctionLikeAst {
    pub fn name_span(&self) -> Span {
        match self {
            FunctionLikeAst::ExternDef(spanned, _) => spanned.data.name.span,
            FunctionLikeAst::Fundef(spanned) => spanned.data.name.span,
            FunctionLikeAst::Method(spanned) => spanned.data.name.span,
            FunctionLikeAst::TraitMethod(spanned) => spanned.data.name.span,
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
            module.data.name.data,
            Some(m_id),
            None,
            vec![],
            parent.package(db),
        ))),
        AstTopLevelItemDesc::Fundef(fundef) => Some(Definition::Function(
            FunctionId::new(db, fundef.data.name.data, ScopeOwnerId::Module(m_id)),
        )),
        AstTopLevelItemDesc::Interface(interface) => Some(Definition::Interface(
            InterfaceId::new(db, interface.name.data, m_id),
        )),
        AstTopLevelItemDesc::Impl(_) => None,
        AstTopLevelItemDesc::StructDef(struct_def) => Some(Definition::Type(
            TypeDefId::Struct(StructId::new(db, struct_def.name.data, m_id)),
        )),
        AstTopLevelItemDesc::EnumDef(ast_enum_def) => Some(Definition::Type(
            TypeDefId::Enum(EnumId::new(db, ast_enum_def.name.data, m_id)),
        )),
        AstTopLevelItemDesc::ExternDef(funsig, _) => Some(Definition::Function(
            FunctionId::new(db, funsig.data.name.data, ScopeOwnerId::Module(m_id)),
        )),
    }
}

#[salsa::tracked]
pub fn builtin_definitions<'db>(db: &'db dyn Db) -> BTreeMap<Symbol, Definition> {
    let mut res = BTreeMap::from([
        (
            Symbol::new(db, "usize"),
            Definition::Type(usize_id(db).def(db)),
        ),
        (Symbol::new(db, "int"), Definition::Type(int_id(db).def(db))),
        (Symbol::new(db, "i32"), Definition::Type(int_id(db).def(db))), // i32 is an alias for int. Might want to switch this around
        (
            Symbol::new(db, "void"),
            Definition::Type(void_id(db).def(db)),
        ),
        (
            Symbol::new(db, "char"),
            Definition::Type(char_id(db).def(db)),
        ),
        (
            Symbol::new(db, "bool"),
            Definition::Type(bool_id(db).def(db)),
        ),
        (
            Symbol::new(db, "never"),
            Definition::Type(never_id(db).def(db)),
        ),
    ]);

    if let Some(std_module) = std_module(db) {
        res.insert(
            Symbol::new(db, "std"),
            Definition::Module(std_module.into()),
        );
    }
    let core_module = core_module(db);
    res.insert(
        Symbol::new(db, "core"),
        Definition::Module(core_module.into()),
    );

    res.insert(Symbol::new(db, "str"), {
        let io = core_module
            .file_submodules(db)
            .into_iter()
            .find(|x| x.name(db) == Symbol::new(db, "io"))
            .unwrap();
        Definition::Type(TypeDefId::Struct(StructId::new(
            db,
            Symbol::new(db, "str"),
            file_module_id(db, io, Some(core_module.into()), core_package(db)),
        )))
    });

    res
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
        Some(fm.file(db)),
        fm.submodules(db).clone(),
        Some(package),
    )
}

impl AstIncludePathDesc {
    pub fn to_segments(&self) -> NonEmpty<Symbol> {
        let (hd, tl) = match self {
            AstIncludePathDesc::Symbol(symbol) => (*symbol, None),
            AstIncludePathDesc::NameResolved { from, to } => (*from, Some(to)),
        };
        fn _to_segments(this: &AstIncludePathDesc, v: &mut NonEmpty<Symbol>) {
            match this {
                AstIncludePathDesc::Symbol(symbol) => {
                    v.push(*symbol);
                }
                AstIncludePathDesc::NameResolved { from, to } => {
                    v.push(*from);
                    _to_segments(&to.data, v);
                }
            }
        }
        let mut res = NonEmpty::singleton(hd);
        if let Some(rest) = tl {
            _to_segments(&rest.data, &mut res);
        }
        res
    }
}

#[salsa::tracked]
pub fn module_definitions<'db>(
    db: &'db dyn Db,
    module: InternedModuleId<'db>,
) -> Vec<(Symbol, Definition)> {
    if module.package(db).is_none() {
        builtin_definitions(db).into_iter().collect()
    } else {
        let mut res = Vec::new();
        for sub in module.file_submodules(db) {
            let id =
                file_module_id(db, sub, Some(module.into()), module.package(db).unwrap());
            res.push((id.name(db), Definition::Module(id)));
        }
        let items = module_items(db, module);
        items.iter().for_each(|items| {
            items.iter().for_each(|item| {
                definition_of_item(db, module, item)
                    .into_iter()
                    .for_each(|def| {
                        res.push((def.name(db), def));
                    })
            })
        });

        res
    }
}

#[salsa::tracked]
pub fn module_includes<'db>(
    db: &'db dyn Db,
    module: InternedModuleId<'db>,
) -> Vec<Segments<'db>> {
    let Some(file) = module.file(db) else {
        return vec![];
    };
    let ast = parse_file(db, file);
    ast.includes(db)
        .iter()
        .map(|include| Segments::new(db, include.data.to_segments()))
        .collect()
}

#[salsa::tracked]
pub fn def_map_in_module<'db>(
    db: &'db dyn Db,
    module: InternedModuleId<'db>,
) -> BTreeMap<Symbol, Definition> {
    module_definitions(db, module).into_iter().collect()
}

fn find_module_in_chain<'db>(
    db: &'db dyn Db,
    name: Symbol,
    module: InternedModuleId<'db>,
) -> Option<ModuleId> {
    let defs = def_map_in_module(db, module);
    if let Some(Definition::Module(m)) = defs.get(&name) {
        return Some(*m);
    }
    module
        .parent(db)
        .and_then(|parent| find_module_in_chain(db, name, parent.interned()))
}

#[salsa::tracked]
pub fn resolve_include_path<'db>(
    db: &'db dyn Db,
    segments: Segments<'db>,
    current: InternedModuleId<'db>,
) -> Option<ModuleId> {
    let v = segments.segments(db);
    let (hd, tl) = v.split_first();
    let head_module = find_module_in_chain(db, *hd, current)?;
    NonEmpty::from_slice(tl).map_or(Some(head_module), |segs| {
        let segs = Segments::new(db, segs);
        resolve_include_path(db, segs, head_module.interned())
    })
}

#[salsa::tracked]
pub fn resolve_in_module<'db>(
    db: &'db dyn Db,
    name: InternedSymbol<'db>,
    module: InternedModuleId<'db>,
) -> Option<Definition> {
    let defs = def_map_in_module(db, module);
    if let Some(def) = defs.get(&Symbol::from(name)) {
        return Some(*def);
    }
    // Check includes declared on this module
    let includes = module_includes(db, module);
    for included_module in includes {
        if let Some(included_id) = resolve_include_path(db, included_module, module)
            && let Some(def) =
                def_map_in_module(db, included_id.interned()).get(&Symbol::from(name))
        {
            return Some(*def);
        }
    }

    // Walk up to parent — this is where parent includes get checked too,
    // since the parent will run its own module_includes when we recurse into it
    module
        .parent(db)
        .and_then(|parent| resolve_in_module(db, name, parent.interned()))
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

#[salsa::tracked]
pub fn get_module_pretty_name<'db>(
    db: &'db dyn Db,
    id: InternedModuleId<'db>,
) -> Arc<String> {
    let prefix = if let Some(parent) = id.parent(db) {
        format!("{}::", get_module_pretty_name(db, parent.interned()))
    } else {
        String::new()
    };
    Arc::new(format!("{}{}", prefix, id.name(db).interned().contents(db)))
}
