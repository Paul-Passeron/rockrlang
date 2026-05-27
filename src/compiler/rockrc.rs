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

use clap::Parser as _;
use clap_derive::Parser;
use rockr::compiler::Config;
use rockr::compiler::check_from_disk;
use std::path::PathBuf;

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
    let cfg = Config {
        no_std: args.no_std,
        skip_core: args.skip_core,
    };
    let root = args
        .file
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    match check_from_disk(root, cfg) {
        Ok(()) => {
            println!("Compilation finished :)");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
