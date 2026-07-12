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

use salsa::Accumulator;

use crate::{
    Db, SourceFile,
    common::{location::Span, symbols::Symbol, unord::Set},
    compiler::{Workspace, diagnostic::Diag, workspace_packages},
    parse_tree::top_level::{Ast, AstTopLevelItem, AstTopLevelItemDesc},
    parser::{ParseError, parse_file},
    ril::{FileModule, InternedModuleId, ModuleId, Package},
};

pub mod definition;
pub mod implems;
pub mod interfaces;
pub mod type_expr;

#[salsa::tracked(returns(copy))]
pub fn module_to_file<'db>(
    db: &'db dyn Db,
    module: InternedModuleId<'db>,
) -> SourceFile {
    module.file(db).unwrap_or_else(|| {
        module_to_file(db, module.parent(db).unwrap().interned())
    })
}

#[salsa::tracked(returns(copy))]
pub fn builtin_module<'db>(db: &'db dyn Db) -> ModuleId {
    ModuleId::new(db, Symbol::new(db, "@builtin"), None, None, vec![], None)
}

/// Build the ModuleId hierarchy for a FileModule tree rooted at a package root.
/// The package root's parent is builtin_module; all submodules are parented to
/// it.
#[salsa::tracked(returns(copy))]
pub fn file_module_id<'db>(
    db: &'db dyn Db,
    file_module: FileModule<'db>,
    parent: Option<ModuleId>,
    package: Package<'db>,
) -> ModuleId {
    let actual_parent = parent.unwrap_or_else(|| builtin_module(db));
    let id = ModuleId::new(
        db,
        file_module.name(db),
        Some(actual_parent),
        Some(*file_module.file(db)),
        file_module.submodules(db).clone(),
        Some(package),
    );
    // Eagerly register submodules so their ModuleIds exist with the right
    // parent
    for sub in file_module.submodules(db) {
        file_module_id(db, *sub, Some(id), package);
    }
    id
}

#[salsa::tracked]
pub fn root_module<'db>(
    db: &'db dyn Db,
    file_module: FileModule<'db>,
    package: Package<'db>,
) -> ModuleId {
    let file = file_module.file(db);
    let full_name = if file.path(db).file_name().unwrap() == "main.rkr" {
        file.path(db)
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string()
    } else {
        file.path(db)
            .with_extension("")
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string()
    };
    let name = Symbol::new(db, full_name);
    ModuleId::new(
        db,
        name,
        Some(builtin_module(db)),
        Some(*file),
        file_module.submodules(db).clone(),
        Some(package),
    )
}

#[allow(dead_code)]
#[salsa::tracked]
pub fn module_items<'db>(
    db: &'db dyn Db,
    module: InternedModuleId<'db>,
) -> Option<Vec<AstTopLevelItem>> {
    if let Some(file) = module.file(db) {
        let ast = parse_file(db, *file);
        let parse_errors: Vec<&ParseError> =
            parse_file::accumulated::<ParseError>(db, *file);
        for err in parse_errors {
            let span = Span::new(err.file, err.start, err.end);
            Diag::generic_error(format!("{:?}", err.kind), span).accumulate(db);
        }
        return Some(ast.items(db).clone());
    }
    module.parent(db).and_then(|parent| {
        match module_items(db, parent.interned()) {
            Some(parent_ast) => {
                parent_ast.iter().find_map(|item| match &item.data {
                    AstTopLevelItemDesc::Module(module_ast) => {
                        if module_ast.data.name.data == *module.name(db) {
                            Some(module_ast.data.items.clone())
                        } else {
                            None
                        }
                    }
                    _ => None,
                })
            }
            None => {
                let file = module_to_file(db, module);
                let ast: Ast<'db> = parse_file(db, file);
                let parse_errors: Vec<&ParseError> =
                    parse_file::accumulated::<ParseError>(db, file);
                for err in parse_errors {
                    let span = Span::new(err.file, err.start, err.end);
                    Diag::generic_error(format!("{:?}", err.kind), span)
                        .accumulate(db);
                }
                Some(ast.items(db).clone())
            }
        }
    })
}

#[salsa::tracked(returns(copy))]
pub fn std_package<'db>(db: &'db dyn Db) -> Option<Package<'db>> {
    if db.config().no_std {
        None
    } else {
        let ws = Workspace::get(db);
        let packages = workspace_packages(db, ws);
        for pkg in packages.iter() {
            if pkg.root(db).name(db).to_string(db) == "std" {
                return Some(*pkg);
            }
        }
        None
    }
}

#[salsa::tracked(returns(copy))]
pub fn std_module<'db>(db: &'db dyn Db) -> Option<ModuleId> {
    let package = std_package(db)?;
    let file_module = package.root(db);
    Some(file_module_id(db, *file_module, Some(builtin_module(db)), package))
}

#[salsa::tracked(returns(copy))]
pub fn core_package<'db>(db: &'db dyn Db) -> Package<'db> {
    let ws = Workspace::get(db);
    let packages = workspace_packages(db, ws);
    for pkg in packages.iter() {
        if pkg.root(db).name(db).to_string(db) == "core" {
            return *pkg;
        }
    }
    unreachable!()
}

#[salsa::tracked(returns(copy))]
pub fn core_module<'db>(db: &'db dyn Db) -> ModuleId {
    let package = core_package(db);
    let file_module = package.root(db);
    file_module_id(db, *file_module, Some(builtin_module(db)), package)
}

fn collect_modules_in_file_module<'db>(
    db: &'db dyn Db,
    file_module: FileModule<'db>,
    module_id: ModuleId,
    package: Package<'db>,
    set: &mut Set<ModuleId>,
) {
    set.insert(module_id);

    for sub in file_module.submodules(db) {
        let sub_id = file_module_id(db, *sub, Some(module_id), package);
        collect_modules_in_file_module(db, *sub, sub_id, package, set);
    }

    if let Some(items) = module_items(db, module_id.interned()) {
        collect_modules_in_items(db, &items, module_id, package, set);
    }
}

fn collect_modules_in_items<'db>(
    db: &'db dyn Db,
    items: &[AstTopLevelItem],
    parent: ModuleId,
    package: Package<'db>,
    set: &mut Set<ModuleId>,
) {
    for item in items {
        if let AstTopLevelItemDesc::Module(module_ast) = &item.data {
            let child_id = ModuleId::new(
                db,
                module_ast.data.name.data,
                Some(parent),
                None,
                vec![],
                Some(package),
            );
            set.insert(child_id);
            collect_modules_in_items(
                db,
                &module_ast.data.items,
                child_id,
                package,
                set,
            );
        }
    }
}

#[salsa::tracked]
pub fn modules_in_package<'db>(
    db: &'db dyn Db,
    package: Package<'db>,
) -> Set<ModuleId> {
    let root_id = file_module_id(db, *package.root(db), None, package);

    let mut set = Set::new();
    collect_modules_in_file_module(
        db,
        *package.root(db),
        root_id,
        package,
        &mut set,
    );
    set
}
