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

use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Compiles,
    Rejected,
    Crashes,
}

const REJECTED: &[&str] =
    &["bad_move", "early_return", "match_tuple", "mut_method", "opt_ref", "partial_move"];

const KNOWN_CRASHES: &[&str] = &[
    "destructure_mut",
    "idx",
    "iter",
    "nested_matching",
    "pattern_stress",
    "round_trip",
    "slice_test",
];

fn expected(name: &str) -> Outcome {
    if REJECTED.contains(&name) {
        Outcome::Rejected
    } else if KNOWN_CRASHES.contains(&name) {
        Outcome::Crashes
    } else {
        Outcome::Compiles
    }
}

fn compile(path: &Path) -> (Outcome, String) {
    let name = path.file_stem().unwrap().to_string_lossy();
    let obj = std::env::temp_dir().join(format!("rockr-codegen-{name}.o"));
    let output = Command::new(env!("CARGO_BIN_EXE_rockrc"))
        .arg(path)
        .args(["-c", "-o"])
        .arg(&obj)
        .output()
        .unwrap_or_else(|e| panic!("failed to run rockrc on {}: {e}", path.display()));
    let _ = std::fs::remove_file(&obj);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let outcome = match output.status.code() {
        Some(0) => Outcome::Compiles,
        Some(1) => Outcome::Rejected,
        _ => Outcome::Crashes,
    };
    (outcome, stderr)
}

#[test]
fn every_example_has_its_expected_outcome() {
    let mut examples: Vec<_> = std::fs::read_dir("examples")
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "rkr"))
        .collect();
    examples.sort();

    let mut failures = vec![];
    for path in &examples {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let want = expected(&name);
        let (got, stderr) = compile(path);
        if got != want {
            let tail: Vec<_> = stderr.lines().rev().take(8).collect();
            let tail: Vec<_> = tail.into_iter().rev().collect();
            failures.push(format!(
                "{name}: expected {want:?}, got {got:?}\n    {}",
                tail.join("\n    ")
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} example(s) changed outcome (update REJECTED / KNOWN_CRASHES in \
         tests/codegen.rs if this is an improvement):\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
