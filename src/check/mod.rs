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

use std::collections::{HashMap, HashSet};

use itertools::Itertools;

use crate::{
    Db,
    check::{
        fundef::check_fundef, implem::check_implem, interface::check_interface,
        types::check_typedef,
    },
    common::{location::Span, symbols::Symbol},
    compiler::{Workspace, diagnostic::Severity, workspace_packages},
    name_resolve::{
        core_module,
        definition::{Definition, module_definitions},
        file_module_id,
        implems::module_impls,
    },
    ril::{FileModule, InternedModuleId, ModuleId, Package},
};

pub mod fundef;
pub mod implem;
pub mod interface;
pub mod types;

#[salsa::tracked]
pub struct Diag<'db> {
    pub severity: Severity,
    pub message: String,
    pub primary: DiagLabel<'db>,
    pub secondary: Vec<DiagLabel<'db>>,
    pub notes: Vec<String>,
    pub help: Vec<String>,
}

#[salsa::tracked]
pub struct DiagLabel<'db> {
    pub span: Span,
    pub message: Option<String>,
}

impl<'db> Diag<'db> {
    pub fn redefinition(db: &'db dyn Db, defs: Vec<Definition>) -> Self {
        assert!(!defs.is_empty());
        let (first, others) = {
            let mut defs = defs;
            let head = defs.remove(0);
            (head, defs)
        };

        let primary = DiagLabel::new(
            db,
            first.name_span(db).unwrap(),
            Some(format!("Defined here")),
        );
        let secondary = others
            .into_iter()
            .map(|def| {
                DiagLabel::new(
                    db,
                    def.name_span(db).unwrap(),
                    Some(format!("Defined here")),
                )
            })
            .collect();

        Diag::new(
            db,
            Severity::Error,
            format!(
                "name `{}` is defined multiple times at top-level.",
                first.name(db).to_string(db)
            ),
            primary,
            secondary,
            vec![],
            vec![],
        )
    }

    pub fn todo(db: &'db dyn Db, message: String, span: Span) -> Self {
        let primary = DiagLabel::new(db, span, Some(message));

        Diag::new(
            db,
            Severity::Warning,
            "not yet implemented.".to_string(),
            primary,
            vec![],
            vec![],
            vec![],
        )
    }
}

#[salsa::tracked]
pub struct Diagnostics<'db> {
    pub diagnostics: Vec<Diag<'db>>,
}

fn print_filemodule<'db>(db: &'db dyn Db, fm: FileModule<'db>, indent: usize) {
    let name = fm.name(db).to_string(db);
    let sms = fm.submodules(db);
    println!(
        "{}{name}{}",
        "    ".repeat(indent),
        if sms.is_empty() { "" } else { ":" }
    );
    for sm in sms {
        print_filemodule(db, *sm, indent + 1);
    }
}

fn print_package<'db>(db: &'db dyn Db, pkg: Package<'db>) {
    print_filemodule(db, pkg.root(db), 0);
}

#[salsa::tracked]
pub fn check<'db>(db: &'db dyn Db, ws: Workspace) -> Diagnostics<'db> {
    let pkgs = workspace_packages(db, ws);
    let mut diagnostics = Vec::new();
    for pkg in &pkgs {
        print_package(db, *pkg);
    }
    for &pkg in pkgs.iter() {
        diagnostics.extend(check_package(db, pkg).diagnostics(db));
    }
    Diagnostics::new(db, diagnostics)
}

pub fn check_definition<'db>(db: &'db dyn Db, def: Definition) -> Diagnostics<'db> {
    match def {
        Definition::Function(function_id) => check_fundef(db, function_id),
        Definition::Interface(interface_id) => check_interface(db, interface_id),
        Definition::Module(module_id) => check_module(db, module_id.interned()),
        Definition::Type(type_def_id) => check_typedef(db, type_def_id),
    }
}

pub fn check_duplicate_defs<'db>(
    db: &'db dyn Db,
    defs: impl Iterator<Item = (Symbol, Definition)>,
) -> Diagnostics<'db> {
    let mut map: HashMap<Symbol, HashSet<Definition>> = HashMap::new();
    for (name, def) in defs {
        map.entry(name).or_default().insert(def);
    }

    let mut diags = vec![];

    for (_, defs) in map {
        if defs.len() > 1 {
            diags.push(Diag::redefinition(
                db,
                defs.into_iter()
                    .sorted_by_key(|k| k.name_span(db).unwrap())
                    .collect(),
            ))
        }
    }

    Diagnostics::new(db, diags)
}

#[salsa::tracked]
pub fn check_module<'db>(db: &'db dyn Db, module: InternedModuleId<'db>) -> Diagnostics<'db> {
    if db.config().skip_core && module == core_module(db) {
        return Diagnostics::new(db, vec![]);
    }
    let mut diags = vec![];
    let defs = module_definitions(db, module);

    let multiple_defs = check_duplicate_defs(db, defs.iter().map(|(a, b)| (*a, *b)));
    diags.extend(multiple_defs.diagnostics(db));

    for def in defs {
        diags.extend(check_definition(db, def.1).diagnostics(db))
    }

    for implem in module_impls(db, module) {
        diags.extend(check_implem(db, implem).diagnostics(db));
    }

    Diagnostics::new(db, diags)
}

#[salsa::tracked]
pub fn check_file_module<'db>(
    db: &'db dyn Db,
    fm: FileModule<'db>,
    pkg: Package<'db>,
    parent: Option<ModuleId>,
) -> Diagnostics<'db> {
    let module = file_module_id(db, fm, parent, pkg);
    check_module(db, module.interned())
}

#[salsa::tracked]
pub fn check_package<'db>(db: &'db dyn Db, pkg: Package<'db>) -> Diagnostics<'db> {
    check_file_module(db, pkg.root(db), pkg, None)
}
