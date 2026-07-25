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
use rockr::compiler::build_from_disk;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[allow(clippy::struct_excessive_bools)]
pub struct CliArgs {
    file: Option<PathBuf>,

    #[clap(long, default_value_t = false)]
    no_std: bool,

    #[clap(long, default_value_t = false)]
    skip_core: bool,

    #[clap(long, default_value_t = false)]
    display_llvm: bool,

    #[clap(long, default_value_t = false)]
    display_opt_llvm: bool,

    #[clap(long, default_value_t = false)]
    display_mir: bool,

    #[clap(long, default_value_t = false)]
    display_thir: bool,

    #[clap(short = 'c', long, default_value_t = false)]
    compile_only: bool,

    #[clap(short = 'o', long)]
    output: Option<PathBuf>,
}

fn main() -> std::process::ExitCode {
    let args = CliArgs::parse();
    let cfg = Config {
        no_std: args.no_std,
        skip_core: args.skip_core,
        display_llvm: args.display_llvm,
        display_mir: args.display_mir,
        display_thir: args.display_thir,
        display_opt_llvm: args.display_opt_llvm,
        compile_only: args.compile_only,
        output: args.output,
    };
    let Some(root) = args.file.or_else(|| {
        std::env::current_dir().inspect_err(|io_err| eprintln!("io error: {io_err}")).ok()
    }) else {
        return std::process::ExitCode::FAILURE;
    };

    match build_from_disk(root, cfg) {
        Ok(()) => {
            // println!("Compilation finished :)");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
