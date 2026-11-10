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
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(10);

fn runnable_examples() -> Vec<PathBuf> {
    let mut examples: Vec<_> = std::fs::read_dir("examples")
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "rkr"))
        .filter(|p| {
            let src = std::fs::read_to_string(p).unwrap();
            src.lines().any(|l| l.starts_with("fun main("))
                && !src.lines().any(|l| l.trim_start().starts_with("//@ no-run"))
        })
        .collect();
    examples.sort();
    examples
}

fn run_with_timeout(mut cmd: Command) -> Result<(Option<i32>, String), String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn: {e}"))?;
    let start = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            let output = child.wait_with_output().map_err(|e| e.to_string())?;
            return Ok((
                output.status.code(),
                String::from_utf8_lossy(&output.stdout).into(),
            ));
        }
        if start.elapsed() > TIMEOUT {
            let _ = child.kill();
            return Err(format!("timed out after {TIMEOUT:?}"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn run_example(path: &Path, workdir: &Path) -> Result<String, String> {
    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    let build = Command::new(env!("CARGO_BIN_EXE_rockrc"))
        .arg(std::fs::canonicalize(path).unwrap())
        .args(["-o", &name])
        .current_dir(workdir)
        .output()
        .map_err(|e| format!("failed to run rockrc: {e}"))?;
    if !build.status.success() {
        return Err(format!(
            "failed to build:\n{}",
            String::from_utf8_lossy(&build.stderr)
        ));
    }
    let mut cmd = Command::new(format!("./{name}"));
    cmd.current_dir(workdir);
    let (code, stdout) = run_with_timeout(cmd)?;
    Ok(format!(
        "{stdout}[exit: {}]\n",
        code.map_or_else(|| "signal".into(), |c| c.to_string())
    ))
}

#[test]
fn examples_run_with_expected_output() {
    let workdir = std::env::temp_dir().join(format!("rockr-run-{}", std::process::id()));
    std::fs::create_dir_all(&workdir).unwrap();
    let update = std::env::var_os("ROCKR_UPDATE_SNAPSHOTS").is_some();

    let mut failures = vec![];
    for path in runnable_examples() {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let snapshot = Path::new("tests/snapshots").join(format!("{name}.stdout"));
        let actual = match run_example(&path, &workdir) {
            Ok(actual) => actual,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        if update {
            std::fs::write(&snapshot, &actual).unwrap();
            continue;
        }
        match std::fs::read_to_string(&snapshot) {
            Ok(expected) if expected == actual => (),
            Ok(expected) => failures.push(format!(
                "{name}: output changed\n--- expected\n{expected}--- actual\n{actual}"
            )),
            Err(_) => failures.push(format!(
                "{name}: missing {} (run with ROCKR_UPDATE_SNAPSHOTS=1)",
                snapshot.display()
            )),
        }
    }
    let _ = std::fs::remove_dir_all(&workdir);

    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}
