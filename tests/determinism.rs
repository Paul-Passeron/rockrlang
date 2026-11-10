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
use std::process::Command;

const RUNS: usize = 3;

fn examples() -> Vec<PathBuf> {
    let mut examples: Vec<_> = ["examples", "examples/fail"]
        .iter()
        .flat_map(|dir| std::fs::read_dir(dir).unwrap())
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "rkr"))
        .collect();
    examples.sort();
    examples
}

fn compile(path: &Path) -> String {
    let name = path.file_stem().unwrap().to_string_lossy();
    let obj = std::env::temp_dir()
        .join(format!("rockr-determinism-{}-{name}.o", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_rockrc"))
        .arg(path)
        .args(["--display-thir", "--display-mir", "-c", "-o"])
        .arg(&obj)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&obj);
    let stderr = String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter(|l| !l.starts_with("thread '"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{}\n--- stderr\n{stderr}\n--- exit {:?}\n",
        String::from_utf8_lossy(&output.stdout),
        output.status.code()
    )
}

fn first_difference(a: &str, b: &str) -> String {
    let (line, (x, y)) =
        a.lines().zip(b.lines()).enumerate().find(|(_, (x, y))| x != y).unwrap_or_else(
            || (a.lines().count().min(b.lines().count()), ("<end>", "<end>")),
        );
    format!("line {}:\n    {x}\n    {y}", line + 1)
}

#[test]
fn compiling_twice_gives_the_same_output() {
    let mut failures = vec![];
    for path in examples() {
        let first = compile(&path);
        for _ in 1..RUNS {
            let again = compile(&path);
            if again != first {
                failures.push(format!(
                    "{}: {}",
                    path.display(),
                    first_difference(&first, &again)
                ));
                break;
            }
        }
    }
    assert!(failures.is_empty(), "nondeterministic output:\n{}", failures.join("\n"));
}
