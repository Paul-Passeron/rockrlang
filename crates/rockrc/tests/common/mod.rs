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

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Root of the Cargo workspace. Integration tests run with the package
/// directory (`crates/rockrc`) as cwd, but `examples/` and the paths printed
/// in snapshots are relative to the workspace root.
pub fn workspace_root() -> &'static Path {
    static ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root should exist")
    });
    &ROOT
}

/// Path to the test-suite directory (where `snapshots/` lives).
#[allow(dead_code)]
pub fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}
