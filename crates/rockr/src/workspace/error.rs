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

pub enum CompilerError {
    NoCompilationUnitFound(PathBuf),
    STDLibNotFound,
    NoFileFoundAt(PathBuf),
    CoreLibNotFound,
    CompiledWithErrors,
    LinkFailed(String),
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCompilationUnitFound(path_buf) => {
                write!(f, "No compilation unit found at `{}`", path_buf.display())
            }
            Self::STDLibNotFound => {
                write!(f, "Standard library (`std`) package not found.")
            }
            Self::NoFileFoundAt(path_buf) => {
                write!(f, "No file found at {}", path_buf.display())
            }
            Self::CoreLibNotFound => {
                write!(f, "Core library (`core`) package not found.")
            }
            Self::CompiledWithErrors => {
                write!(f, "Errors encountered, did not compile.")
            }
            Self::LinkFailed(msg) => {
                write!(f, "Linking failed: {msg}")
            }
        }
    }
}
