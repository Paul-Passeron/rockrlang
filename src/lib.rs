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

#![feature(option_into_flat_iter, vec_try_remove)]

use std::path::Path;
use std::process::ExitCode;

pub use common::location::SourceFile;
pub use db::*;

pub mod check;
pub mod common;
pub mod compiler;
pub mod db;
pub mod driver;
pub mod hir;
pub mod lexer;
pub mod name_resolve;
pub mod parse_tree;
pub mod parser;
pub mod printer;
pub mod ril;
pub mod tests;
pub mod thir;
pub mod typecheck;

#[derive(Debug)]
pub struct RunStatus {
    pub exit_code: ExitCode,
    pub stderr: String,
    pub stdout: String,
}

pub fn run_rkr(p: &Path) -> RunStatus {
    let stderr = String::new();
    let stdout = String::new();
    let cfg = compiler::Config::default();

    let exit_code = match compiler::check_from_disk(p.to_path_buf(), cfg) {
        Ok(()) => {
            println!("Compilation finished :)");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    };
    RunStatus {
        exit_code,
        stderr,
        stdout,
    }
}
