use clap::Parser;
use clap_derive::Parser;
pub use common::location::OwnedSourceFile;
pub use common::location::SourceFile;
pub use db::*;
use std::path::PathBuf;

mod common;
mod compiler;
mod db;
mod driver;
mod hir;
mod lexer;
mod name_resolve;
mod parse_tree;
mod parser;
mod printer;
mod ril;
mod tests;
mod thir;

#[derive(Debug, Parser)]
pub struct CliArgs {
    file: Option<PathBuf>,
    #[clap(long, default_value_t = false)]
    no_std: bool,
    #[clap(long, default_value_t = false)]
    skip_core: bool,
}

fn main() -> std::process::ExitCode {
    let args = CliArgs::parse();
    let cfg = compiler::Config {
        no_std: args.no_std,
        skip_core: args.skip_core,
    };
    let root = args
        .file
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    match compiler::check(&root, cfg) {
        Ok(report) => {
            printer::print(&report);
            if report.has_errors() {
                std::process::ExitCode::FAILURE
            } else {
                std::process::ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
