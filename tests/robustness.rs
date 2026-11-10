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

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(120);
const LINE_DELETIONS_PER_FILE: usize = 8;

const KNOWN_PANICS: &[(&str, &str)] = &[
    ("", "Cannot index into"),
    ("slice_test.rkr", "not yet implemented"),
    ("", "Non-concrete type has reached MIR"),
    ("", "`Self` used outside of an impl"),
];

fn is_known(origin: &str, msg: &str) -> bool {
    KNOWN_PANICS.iter().any(|(file, k)| origin.starts_with(file) && msg.contains(k))
}

struct Mutant {
    origin: String,
    source: String,
}

fn mutants() -> Vec<Mutant> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir("examples")
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "rkr"))
        .collect();
    paths.sort();

    let mut out = vec![];
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let src = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = src.lines().collect();
        let mut push = |what: String, source: String| {
            out.push(Mutant { origin: format!("{name} ({what})"), source });
        };

        for quarter in 1..4 {
            let keep = lines.len() * quarter / 4;
            push(format!("first {keep} lines"), lines[..keep].join("\n"));
        }

        let step = (lines.len() / LINE_DELETIONS_PER_FILE).max(1);
        for i in (0..lines.len()).step_by(step) {
            let mut l = lines.clone();
            l.remove(i);
            push(format!("line {} deleted", i + 1), l.join("\n"));
        }

        for needle in ["}", ";", ")", "{"] {
            if let Some(pos) = src.find(needle) {
                let mut s = src.clone();
                s.remove(pos);
                push(format!("first `{needle}` deleted"), s);
            }
        }
    }
    out
}

enum Verdict {
    Ok,
    Panicked(String),
    TimedOut(PathBuf),
}

fn compile(source: &str, workdir: &std::path::Path, id: usize) -> Verdict {
    let file = workdir.join(format!("m{id}.rkr"));
    let obj = workdir.join(format!("m{id}.o"));
    let err = workdir.join(format!("m{id}.stderr"));
    std::fs::write(&file, source).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rockrc"))
        .arg(&file)
        .args(["-c", "-o"])
        .arg(&obj)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&err).unwrap())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            let keep = PathBuf::from("target/robustness-timeouts");
            std::fs::create_dir_all(&keep).unwrap();
            let saved = keep.join(format!("m{id}.rkr"));
            std::fs::copy(&file, &saved).unwrap();
            let _ = std::fs::copy(&err, keep.join(format!("m{id}.stderr")));
            return Verdict::TimedOut(saved);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let stderr = String::from_utf8_lossy(&std::fs::read(&err).unwrap()).into_owned();
    let _ = std::fs::remove_file(&err);
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_file(&obj);
    if status.code() == Some(101) {
        let mut lines = stderr.lines().skip_while(|l| !l.starts_with("thread '"));
        lines.next();
        Verdict::Panicked(lines.next().unwrap_or("<no message>").to_owned())
    } else {
        Verdict::Ok
    }
}

#[test]
fn damaged_examples_never_crash_the_compiler() {
    let mutants = mutants();
    let workdir =
        std::env::temp_dir().join(format!("rockr-robustness-{}", std::process::id()));
    std::fs::create_dir_all(&workdir).unwrap();

    let next = AtomicUsize::new(0);
    let problems: Mutex<BTreeMap<String, Vec<String>>> = Mutex::new(BTreeMap::new());
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(m) = mutants.get(i) else { break };
                    let key = match compile(&m.source, &workdir, i) {
                        Verdict::Ok => continue,
                        Verdict::TimedOut(saved) => {
                            format!(
                                "timed out after {TIMEOUT:?}, input saved to {}",
                                saved.display()
                            )
                        }
                        Verdict::Panicked(msg) => {
                            if is_known(&m.origin, &msg) {
                                continue;
                            }
                            msg
                        }
                    };
                    problems
                        .lock()
                        .unwrap()
                        .entry(key)
                        .or_default()
                        .push(m.origin.clone());
                }
            });
        }
    });
    let _ = std::fs::remove_dir_all(&workdir);

    let problems = problems.into_inner().unwrap();
    let report: Vec<String> = problems
        .iter()
        .map(|(msg, origins)| {
            let mut origins = origins.clone();
            origins.sort();
            format!(
                "{msg}\n    e.g. {}",
                origins.iter().take(3).cloned().collect::<Vec<_>>().join("\n         ")
            )
        })
        .collect();
    assert!(
        problems.is_empty(),
        "{} of {} damaged inputs crashed or hung the compiler:\n\n{}",
        problems.values().map(Vec::len).sum::<usize>(),
        mutants.len(),
        report.join("\n\n")
    );
}
