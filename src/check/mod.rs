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

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use itertools::Itertools;
use salsa::Accumulator;

use crate::{
    Db,
    check::{
        fundef::{check_fundef, reachable_mir_instances},
        implem::check_implem,
        interface::check_interface,
        types::check_typedef,
    },
    common::symbols::Symbol,
    compiler::{Workspace, diagnostic::Diag, workspace_packages},
    name_resolve::{
        core_module,
        definition::{Definition, module_definitions},
        file_module_id,
        implems::module_impls,
    },
    ril::{FileModule, FunctionId, InternedModuleId, ModuleId, Package},
    thir_to_mir::{FuncInst, MIRKey, mir},
};
pub mod fundef;
pub mod implem;
pub mod interface;
pub mod mir;
pub mod thir;
pub mod types;

#[salsa::tracked]
pub fn check<'db>(db: &'db dyn Db, ws: Workspace) {
    let pkgs = workspace_packages(db, ws);
    for &pkg in pkgs.iter() {
        check_package(db, pkg)
    }
}

pub fn check_definition(db: &dyn Db, def: Definition) {
    match def {
        Definition::Function(function_id) => check_fundef(db, function_id),
        Definition::Interface(interface_id) => {
            check_interface(db, interface_id)
        }
        Definition::Module(module_id) => check_module(db, module_id.interned()),
        Definition::Type(type_def_id) => check_typedef(db, type_def_id),
    }
}

pub fn check_duplicate_defs(
    db: &dyn Db,
    defs: impl Iterator<Item = (Symbol, Definition)>,
) {
    let mut map: HashMap<Symbol, HashSet<Definition>> = HashMap::new();
    for (name, def) in defs {
        map.entry(name).or_default().insert(def);
    }

    for (_, defs) in map {
        if defs.len() > 1 {
            Diag::redefinition(
                db,
                defs.into_iter()
                    .sorted_by_key(|k| k.name_span(db).unwrap())
                    .collect(),
            )
            .accumulate(db);
        }
    }
}

#[salsa::tracked]
pub fn check_module<'db>(db: &'db dyn Db, module: InternedModuleId<'db>) {
    // Skip the core module if config says so
    if db.config().skip_core && module == core_module(db) {
        return;
    }
    let defs = module_definitions(db, module);
    check_duplicate_defs(db, defs.iter().map(|(a, b)| (*a, *b)));
    for def in defs {
        check_definition(db, def.1);
    }
    for implem in module_impls(db, module) {
        check_implem(db, implem);
    }
}

#[salsa::tracked]
pub fn check_file_module<'db>(
    db: &'db dyn Db,
    fm: FileModule<'db>,
    pkg: Package<'db>,
    parent: Option<ModuleId>,
) {
    let module = file_module_id(db, fm, parent, pkg);
    check_module(db, module.interned());
}

fn collect_module_functions<'db>(
    db: &'db dyn Db,
    module: InternedModuleId<'db>,
) -> Vec<FunctionId> {
    let mut funcs = vec![];
    for (_, def) in module_definitions(db, module) {
        match def {
            Definition::Function(function_id) => funcs.push(function_id),
            Definition::Module(module_id) => {
                funcs
                    .extend(collect_module_functions(db, module_id.interned()));
            }
            Definition::Interface(_) | Definition::Type(_) => (),
        }
    }
    funcs
}

#[salsa::tracked]
pub fn reachable_frefs<'db>(
    db: &'db dyn Db,
    pkg: Package<'db>,
) -> Arc<Vec<FuncInst>> {
    let module = file_module_id(db, pkg.root(db), None, pkg);
    let roots = collect_module_functions(db, module.interned());

    let mut seen: HashSet<MIRKey> = HashSet::new();
    let mut frefs: Vec<FuncInst> = vec![];

    for root in roots {
        for (fdef, subs) in reachable_mir_instances(db, root) {
            if !seen.insert(MIRKey::new(db, fdef, subs.clone())) {
                continue;
            }
            if fdef.has_body(db) {
                let the_mir = mir(db, fdef, subs);
                frefs.push(the_mir.func);
            } else {
                frefs.push(MIRKey::new(db, fdef, subs).into());
            }
        }
    }
    Arc::new(frefs)
}

#[salsa::tracked]
pub fn check_package<'db>(db: &'db dyn Db, pkg: Package<'db>) {
    check_file_module(db, pkg.root(db), pkg, None);
}
