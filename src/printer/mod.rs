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

use std::{fmt, path::PathBuf};

use crate::{
    common::location::compute_loc_info,
    compiler::{FileModuleInfo, FunctionResult, PackageInfo, Report, diagnostic::Diagnostic},
};

pub mod type_printer;

fn print_file_module(
    f: &mut impl fmt::Write,
    file_module: &FileModuleInfo,
    indent: usize,
) -> std::fmt::Result {
    let prefix = "    ".repeat(indent);
    writeln!(
        f,
        "{prefix}[{}] {}",
        file_module.name(),
        file_module.file.path.display()
    )?;
    file_module
        .submodules
        .iter()
        .try_for_each(|submodule| print_file_module(f, submodule, indent + 1))
}

fn get_package_path_from_env_var(env_var: &str) -> Option<PathBuf> {
    let path = std::env::var(env_var).ok()?;
    let root = std::path::Path::new(&path).join("main.rkr");
    // let root = root.canonicalize().unwrap_or(root);
    Some(root)
}

fn is_builtin_file_module(file_module: &FileModuleInfo) -> bool {
    let p = file_module.file.path.to_path_buf();
    // let p = p.canonicalize().unwrap_or(p);

    let builtin_modules = [
        get_package_path_from_env_var("ROCKR_STD").unwrap_or_default(),
        get_package_path_from_env_var("ROCKR_CORE").unwrap_or_default(),
    ];

    for module in builtin_modules {
        if p == module {
            return true;
        }
    }

    false
}

fn print_packages_structure<'a>(
    f: &mut impl fmt::Write,
    packages: impl IntoIterator<Item = &'a PackageInfo>,
) -> std::fmt::Result {
    for p in packages {
        if is_builtin_file_module(&p.root) {
            continue;
        }
        print_file_module(f, &p.root, 0)?;
    }
    Ok(())
}

pub fn print_diagnostic(f: &mut impl std::fmt::Write, diag: &Diagnostic) -> std::fmt::Result {
    let span_info = diag.primary.span.clone();
    let loc_info = compute_loc_info(
        &span_info.file.content,
        span_info.start,
        span_info.file.path,
    );
    writeln!(f, "{}: {}", diag.severity, diag.message)?;
    writeln!(
        f,
        "| {loc_info}: {}",
        diag.primary
            .message
            .as_ref()
            .cloned()
            .unwrap_or("[Error here]".to_string())
    )
}

pub fn print_function_result(
    f: &mut impl std::fmt::Write,
    result: &FunctionResult,
) -> std::fmt::Result {
    writeln!(f, "[FUNC]======================")?;
    writeln!(f, "{}", result.name)?;
    if !result.hir.is_empty() {
        writeln!(f, "[HIR]=======================")?;
        write!(f, "{}", result.hir)?;
        writeln!(f, "[EXPRS]=====================")?;
        for (expr, ty) in &result.typed_exprs {
            writeln!(f, "{expr:?} => {ty}")?;
        }
        writeln!(f, "[LOCALS]====================")?;
        for (local, ty) in &result.locals {
            writeln!(f, "_{} => {ty}", local.0)?;
        }
    }
    writeln!(f, "[DIAGS]=====================")?;
    for diag in &result.diagnostics {
        print_diagnostic(f, diag)?;
    }
    writeln!(f, "============================\n")
}

pub fn print_to_writer(f: &mut impl fmt::Write, report: &Report) -> std::fmt::Result {
    print_packages_structure(f, &report.packages)?;

    report
        .funcs
        .iter()
        .try_for_each(|func| print_function_result(f, func))
}

impl ToString for Report {
    fn to_string(&self) -> String {
        let mut s = String::new();
        print_to_writer(&mut s, self).unwrap();
        s
    }
}

pub fn print(report: &Report) {
    println!("{}", report.to_string())
}
