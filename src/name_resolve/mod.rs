use crate::{
    Db, SourceFile,
    common::symbols::Symbol,
    parse_tree::top_level::{Ast, AstTopLevelItem, AstTopLevelItemDesc},
    parser::parse_file,
    ril::{FileModule, InternedModuleId, ModuleId},
};

pub mod definition;

#[salsa::tracked]
pub fn module_to_file<'db>(db: &'db dyn Db, module: InternedModuleId<'db>) -> SourceFile<'db> {
    match module.file(db) {
        Some(file) => file.to_source_file(db),
        None => module_to_file(db, module.parent(db).unwrap().interned()),
    }
}

#[salsa::tracked]
pub fn builtin_module<'db>(db: &'db dyn Db) -> ModuleId {
    ModuleId::new(db, Symbol::new(db, "@builtin"), None, None)
}

/// Build the ModuleId hierarchy for a FileModule tree rooted at a package root.
/// The package root's parent is builtin_module; all submodules are parented to it.
#[salsa::tracked]
pub fn file_module_id<'db>(
    db: &'db dyn Db,
    file_module: FileModule<'db>,
    parent: Option<ModuleId>,
) -> ModuleId {
    let actual_parent = parent.unwrap_or_else(|| builtin_module(db));
    let id = ModuleId::new(
        db,
        file_module.name(db),
        Some(actual_parent),
        Some(file_module.file(db).to_owned(db)),
    );
    // Eagerly register submodules so their ModuleIds exist with the right parent
    for sub in file_module.submodules(db) {
        file_module_id(db, *sub, Some(id));
    }
    id
}

#[salsa::tracked]
pub fn root_module<'db>(db: &'db dyn Db, file: SourceFile<'db>) -> ModuleId {
    let full_name = if file.path(db).file_name().unwrap() == "main.rkr" {
        file.path(db)
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string()
    } else {
        let full_name = file
            .path(db)
            .with_extension("")
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        full_name
    };
    let name = Symbol::new(db, full_name);
    ModuleId::new(db, name, Some(builtin_module(db)), Some(file.to_owned(db)))
}

#[allow(dead_code)]
#[salsa::tracked]
pub fn module_items<'db>(
    db: &'db dyn Db,
    module: InternedModuleId<'db>,
) -> Option<Vec<AstTopLevelItem>> {
    if let Some(file) = module.file(db) {
        let ast = parse_file(db, file.to_source_file(db));
        return Some(ast.items(db).clone());
    }
    module
        .parent(db)
        .map(|parent| match module_items(db, parent.interned()) {
            Some(parent_ast) => parent_ast.iter().find_map(|item| match &item.data {
                AstTopLevelItemDesc::Module(module_ast) => {
                    if module_ast.data.name == module.name(db) {
                        Some(module_ast.data.items.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            }),
            None => {
                let file = module_to_file(db, module);
                let ast: Ast<'db> = parse_file(db, file);
                Some(ast.items(db).clone())
            }
        })
        .flatten()
}
