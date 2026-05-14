use crate::Db;
use crate::name_resolve::find_module_in_ast::find_module_in_ast;
use crate::parser::parse_file;
use crate::{parse_tree::top_level::AstTopLevelItem, ril::InternedModuleId};

pub mod find_module_in_ast;

#[salsa::tracked]
fn module_items<'db>(db: &'db dyn Db, module: InternedModuleId<'db>) -> Vec<AstTopLevelItem> {
    let module_ast = find_module_in_ast(db, module);
    if let Some(module_ast) = module_ast {
        module_ast.data.items.clone()
    } else {
        let ast = parse_file(db, module.file(db).unwrap().to_source_file(db));
        ast.items(db).clone()
    }
}
