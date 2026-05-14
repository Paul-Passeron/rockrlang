use crate::{
    Db,
    common::symbols::{InternedSymbol, Symbol},
    name_resolve::{core_module, module_items, std_module},
    parse_tree::top_level::{AstIncludePathDesc, AstTopLevelItem, AstTopLevelItemDesc},
    parser::parse_file,
    ril::{
        FileModule, FunctionId, InterfaceId, InternedModuleId, ModuleId, Package, ScopeOwnerId,
        StructId, TypeDefId, char_id,
        display::{Display, RilDisplay},
        int_id, str_id, void_id,
    },
};
use nonempty::NonEmpty;
use std::sync::Arc;
use std::{collections::HashMap, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    let mut res = HashMap::from([
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
    ]);

    if let Some(std_module) = std_module(db) {
        res.insert(
            Symbol::new(db, "std"),
            Definition::Module(std_module.into()),
        );
    }
    res.insert(
        Symbol::new(db, "core"),
        Definition::Module(core_module(db).into()),
    );

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
        Some(fm.file(db).to_owned(db)),
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
) -> HashMap<Symbol, Definition> {
    if module.package(db).is_none() {
        builtin_definitions(db)
    } else {
        let mut res = HashMap::new();
        for sub in module.file_submodules(db) {
            let id = file_module_id(db, sub, Some(module.into()), module.package(db).unwrap());
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
pub fn module_includes<'db>(db: &'db dyn Db, module: InternedModuleId<'db>) -> Vec<Segments<'db>> {
    let Some(file) = module.file(db) else {
        return vec![];
    };
    let ast = parse_file(db, file.to_source_file(db));
    ast.includes(db)
        .iter()
        .map(|include| Segments::new(db, include.data.to_segments()))
        .collect()
}

fn find_module_in_chain<'db>(
    db: &'db dyn Db,
    name: Symbol,
    module: InternedModuleId<'db>,
) -> Option<ModuleId> {
    let defs = module_definitions(db, module);
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
    let defs = module_definitions(db, module);
    if let Some(def) = defs.get(&Symbol::from(name)) {
        return Some(def.clone());
    }
    // Check includes declared on this module
    let includes = module_includes(db, module);
    for included_module in includes {
        if let Some(included_id) = resolve_include_path(db, included_module, module) {
            if let Some(def) =
                module_definitions(db, included_id.interned()).get(&Symbol::from(name))
            {
                return Some(def.clone());
            }
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
pub fn get_module_pretty_name<'db>(db: &'db dyn Db, id: InternedModuleId<'db>) -> Arc<String> {
    let prefix = if let Some(parent) = id.parent(db) {
        format!("{}::", get_module_pretty_name(db, parent.interned()))
    } else {
        format!("")
    };
    Arc::new(format!("{}{}", prefix, id.name(db).interned().contents(db)))
}

impl RilDisplay for Definition {}

impl fmt::Display for Display<'_, Definition> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.value.name(self.db).interned().contents(self.db)
        )
    }
}
