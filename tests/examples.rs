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

use libtest_mimic::{Arguments, Failed, Trial};
use rockr::{self, RunStatus, run_rkr};
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};
use walkdir::WalkDir;

fn main() {
    let args = Arguments::from_args();

    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let fail_dir = examples_dir.join("fail");

    let mut trials = Vec::new();

    for path in rkr_files_in(&examples_dir, false) {
        let name = test_name(&examples_dir, &path);
        let snap_name = name.clone();
        trials.push(Trial::test(name, move || run_pass_case(&path, &snap_name)));
    }

    for path in rkr_files_in(&fail_dir, true) {
        let name = test_name(&examples_dir, &path);
        let snap_name = name.clone();
        trials.push(Trial::test(name, move || run_fail_case(&path, &snap_name)));
    }

    libtest_mimic::run(&args, trials).exit();
}

fn rkr_files_in(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let walker =
        if recursive { WalkDir::new(dir) } else { WalkDir::new(dir).max_depth(1) };
    let mut files: Vec<PathBuf> = walker
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "rkr"))
        .collect();
    files.sort();
    files
}

fn test_name(examples_dir: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(examples_dir).unwrap_or(path);
    let stem = rel.with_extension("");
    stem.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("::")
}
fn run_pass_case(path: &Path, snap_name: &str) -> Result<(), Failed> {
    let output = run_rkr(path);

    if !matches!(output.exit_code, ExitCode::SUCCESS) {
        return Err(format!(
            "expected success, got {:?}\nstderr:\n{}",
            output.exit_code, output.stderr
        )
        .into());
    }

    snapshot(snap_name, &output);
    Ok(())
}

fn run_fail_case(path: &Path, snap_name: &str) -> Result<(), Failed> {
    let output = run_rkr(path);

    if matches!(output.exit_code, ExitCode::SUCCESS) {
        return Err(format!(
            "expected failure, but program ran successfully\nstdout:\n{}",
            output.stdout
        )
        .into());
    }

    snapshot(snap_name, &output);
    Ok(())
}
fn snapshot(snap_name: &str, output: &RunStatus) {
    insta::with_settings!({
        snapshot_path => "snapshots",
        prepend_module_to_snapshot => false,
        filters => vec![
            // Strip absolute paths up to the examples dir.
            (r"[^\s]*?/examples/", "[EXAMPLES]/"),
        ],
    }, {
        insta::assert_snapshot!(snap_name, format!("stdout:\n{}\n\nstderr:\n{}",output.stdout,output.stderr));
    });
}
