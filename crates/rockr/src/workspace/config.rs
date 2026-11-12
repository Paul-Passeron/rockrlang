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

use clap_derive::ValueEnum;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum OptLevel {
    #[value(name = "0")]
    O0,
    #[value(name = "1")]
    O1,
    #[default]
    #[value(name = "2")]
    O2,
    #[value(name = "3")]
    O3,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub no_std: bool,
    pub skip_core: bool,
    pub display_llvm: bool,
    pub display_opt_llvm: bool,
    pub display_mir: bool,
    pub display_thir: bool,
    pub compile_only: bool,
    pub show_time: bool,
    pub opt_level: OptLevel,
    pub output: Option<PathBuf>,
}
