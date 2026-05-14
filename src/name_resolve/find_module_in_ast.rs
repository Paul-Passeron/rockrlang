use crate::{
    Db, SourceFile,
    common::symbols::Symbol,
    parse_tree::top_level::{
        Ast, AstAnyTopLevelItem, AstAnyTopLevelItemDesc, AstModule, AstTopLevelItem,
        AstTopLevelItemDesc,
    },
    parser::parse_file,
    ril::{InternedModuleId, ModuleId},
};

#[salsa::tracked]
pub fn module_to_file<'db>(db: &'db dyn Db, module: InternedModuleId<'db>) -> SourceFile<'db> {
    match module.file(db) {
        Some(file) => file.to_source_file(db),
        None => module_to_file(db, module.parent(db).unwrap().interned()),
    }
}

#[salsa::tracked]
pub fn root_module<'db>(db: &'db dyn Db, file: SourceFile<'db>) -> ModuleId {
    let full_name = file
        .path(db)
        .with_extension("")
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let name = Symbol::new(db, full_name);
    ModuleId::new(db, name, None, Some(file.to_owned(db)))
}

trait IsModule {
    fn is_module<'db>(&'db self) -> Option<&'db AstModule>;
}

impl IsModule for AstAnyTopLevelItem {
    fn is_module<'db>(&'db self) -> Option<&'db AstModule> {
        match &self.data {
            AstAnyTopLevelItemDesc::Include(_) => None,
            AstAnyTopLevelItemDesc::Item(item) => item.is_module(),
        }
    }
}

impl IsModule for AstTopLevelItemDesc {
    fn is_module<'db>(&'db self) -> Option<&'db AstModule> {
        match self {
            AstTopLevelItemDesc::Module(module) => Some(module),
            _ => None,
        }
    }
}

impl IsModule for AstTopLevelItem {
    fn is_module<'db>(&'db self) -> Option<&'db AstModule> {
        self.data.is_module()
    }
}

fn get_named_module<'db, T: IsModule>(name: Symbol, items: &'db [T]) -> Option<&'db AstModule> {
    items.iter().find_map(|item| {
        if let Some(module) = item.is_module()
            && module.data.name == name
        {
            Some(module)
        } else {
            None
        }
    })
}

#[allow(dead_code)]
pub fn find_module_in_ast<'db>(
    db: &'db dyn Db,
    module: InternedModuleId<'db>,
) -> Option<&'db AstModule> {
    module
        .parent(db)
        .map(|parent| match find_module_in_ast(db, parent.interned()) {
            Some(parent_ast) => get_named_module(module.name(db), &parent_ast.data.items),
            None => {
                let file = module_to_file(db, module);
                let ast: Ast<'db> = parse_file(db, file);
                get_named_module(module.name(db), ast.items(db))
            }
        })
        .flatten()
}
