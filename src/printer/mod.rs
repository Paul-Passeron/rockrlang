use std::path::PathBuf;

use crate::compiler::{
    FileModuleInfo, FunctionResult, PackageInfo, Report, diagnostic::Diagnostic,
};

fn print_file_module(file_module: &FileModuleInfo, indent: usize) {
    let prefix = "    ".repeat(indent);
    println!(
        "{prefix}[{}] {}",
        file_module.name(),
        file_module.file.path.display()
    );
    file_module
        .submodules
        .iter()
        .for_each(|submodule| print_file_module(submodule, indent + 1));
}

fn get_package_path_from_env_var(env_var: &str) -> Option<PathBuf> {
    let path = std::env::var(env_var).ok()?;
    let root = std::path::Path::new(&path).join("main.rkr");
    let root = root.canonicalize().unwrap_or(root);
    Some(root)
}

fn is_builtin_file_module(file_module: &FileModuleInfo) -> bool {
    let p = file_module.file.path.to_path_buf();
    let p = p.canonicalize().unwrap_or(p);

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

fn print_packages_structure<'a>(packages: impl IntoIterator<Item = &'a PackageInfo>) {
    for p in packages {
        if is_builtin_file_module(&p.root) {
            continue;
        }
        print_file_module(&p.root, 0);
    }
}

pub fn print_diagnostic(_diag: &Diagnostic) {
    todo!()
}

pub fn print_function_result(result: &FunctionResult) {
    println!("[FUNC]======================");
    println!("{}", result.name);
    if !result.hir.is_empty() {
        println!("[HIR]=======================");
        print!("{}", result.hir);
        println!("[EXPRS]=====================");
        for (expr, ty) in &result.typed_exprs {
            println!("{expr:?} => {ty}")
        }
        println!("[LOCALS]====================");
        for (local, ty) in &result.locals {
            println!("_{} => {ty}", local.0)
        }
    }
    println!("[DIAGS]=====================");
    for diag in &result.diagnostics {
        print_diagnostic(diag)
    }
    println!("============================\n");
}

pub fn print(report: &Report) {
    print_packages_structure(&report.packages);
    report
        .funcs
        .iter()
        .for_each(|func| print_function_result(func));
}
