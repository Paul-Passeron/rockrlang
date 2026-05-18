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

#![feature(bool_to_result, option_into_flat_iter)]

use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

pub use common::location::OwnedSourceFile;
pub use common::location::SourceFile;
pub use db::*;

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

#[derive(Debug)]
pub struct RunStatus {
    pub exit_code: ExitCode,
    pub stderr: String,
    pub stdout: String,
}

pub fn run_rkr(p: &Path) -> RunStatus {
    let mut stderr = String::new();
    let mut stdout = String::new();
    let cfg = compiler::Config {
        no_std: false,
        skip_core: false,
    };

    let exit_code = match compiler::check(p, cfg) {
        Ok(report) => {
            write!(&mut stdout, "{}", report.to_string()).unwrap();
            if report.has_errors() {
                writeln!(&mut stderr, "Could not compile, errors were encountered.").unwrap();
                std::process::ExitCode::FAILURE
            } else {
                std::process::ExitCode::SUCCESS
            }
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
