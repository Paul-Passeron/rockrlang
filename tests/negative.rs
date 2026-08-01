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

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use rockr::check::check;
use rockr::common::location::LocationInfo;
use rockr::compiler::diagnostic::Severity;
use rockr::compiler::{Config, OptLevel, Workspace, load_workspace_from_disk};

const FAIL_DIR: &str = "examples/fail";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Expected {
    line: usize,
    severity: Severity,
    needle: String,
    written_on: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Reported {
    line: usize,
    column: usize,
    severity: Severity,
    message: String,
}

#[derive(Debug, Default)]
struct Directives {
    ignore: Option<String>,
    allow_extra: bool,
    no_std: bool,
}

fn parse_severity(word: &str) -> Option<Severity> {
    match word {
        "ERROR" => Some(Severity::Error),
        "WARNING" => Some(Severity::Warning),
        "NOTE" => Some(Severity::Note),
        "HELP" => Some(Severity::Help),
        _ => None,
    }
}

fn parse_annotations(source: &str) -> Result<(Directives, Vec<Expected>), String> {
    let mut directives = Directives::default();
    let mut expectations: Vec<Expected> = Vec::new();

    for (idx, text) in source.lines().enumerate() {
        let written_on = idx + 1;

        if let Some(rest) = text.split("//@").nth(1) {
            let rest = rest.trim();
            if let Some(reason) = rest.strip_prefix("ignore:") {
                directives.ignore = Some(reason.trim().to_owned());
            } else if rest == "allow-extra" {
                directives.allow_extra = true;
            } else if rest == "no-std" {
                directives.no_std = true;
            } else {
                return Err(format!("line {written_on}: unknown directive `//@ {rest}`"));
            }
        }

        for rest in text.split("//~").skip(1) {
            let carets = rest.chars().take_while(|c| *c == '^').count();
            let continuation = carets == 0 && rest.starts_with('|');
            let rest = &rest[carets + usize::from(continuation)..];

            let line = if continuation {
                expectations
                    .last()
                    .ok_or_else(|| {
                        format!("line {written_on}: `//~|` with no preceding annotation")
                    })?
                    .line
            } else {
                written_on.checked_sub(carets).filter(|l| *l > 0).ok_or_else(|| {
                    format!(
                        "line {written_on}: `//~` points {carets} lines up, past the top"
                    )
                })?
            };

            let rest = rest.trim_start();
            let (word, needle) =
                rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let severity = parse_severity(word).ok_or_else(|| {
                format!(
                    "line {written_on}: expected ERROR / WARNING / NOTE / HELP, \
                     got `{word}`"
                )
            })?;

            expectations.push(Expected {
                line,
                severity,
                needle: needle.trim().to_owned(),
                written_on,
            });
        }
    }

    Ok((directives, expectations))
}

fn config(no_std: bool) -> Config {
    Config {
        no_std,
        skip_core: true,
        display_llvm: false,
        display_opt_llvm: false,
        display_mir: false,
        display_thir: false,
        compile_only: true,
        show_time: false,
        opt_level: OptLevel::O0,
        output: None,
    }
}

fn diagnostics_for(path: &Path, no_std: bool) -> Result<Vec<Reported>, String> {
    let db = load_workspace_from_disk(path.to_path_buf(), config(no_std))
        .map_err(|_| "failed to load the workspace".to_owned())?;
    let ws = Workspace::get(&db);
    check(&db, ws);

    let want = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut reported: Vec<Reported> = rockr::compiler::program_has_errors(&db)
        .1
        .into_iter()
        .filter_map(|diag| {
            let LocationInfo { file, line, column, .. } =
                diag.primary.span.start().loc_info(&db);
            let same_file =
                file.canonicalize().map_or(file == &want, |file| file == want);
            same_file.then(|| Reported {
                line: *line,
                column: *column,
                severity: diag.severity,
                message: diag.message.clone(),
            })
        })
        .collect();

    reported.sort_by(|a, b| {
        (a.line, a.column, a.severity, &a.message)
            .cmp(&(b.line, b.column, b.severity, &b.message))
    });
    Ok(reported)
}

fn compare(
    expectations: &[Expected],
    reported: &[Reported],
) -> (Vec<Expected>, Vec<Reported>) {
    let mut claimed: BTreeSet<usize> = BTreeSet::new();
    let mut unmet = Vec::new();

    for expected in expectations {
        let hit = reported.iter().enumerate().find(|(i, got)| {
            !claimed.contains(i)
                && got.line == expected.line
                && got.severity == expected.severity
                && got.message.contains(&expected.needle)
        });
        match hit {
            Some((i, _)) => {
                claimed.insert(i);
            }
            None => unmet.push(expected.clone()),
        }
    }

    let extra = reported
        .iter()
        .enumerate()
        .filter(|(i, _)| !claimed.contains(i))
        .map(|(_, got)| got.clone())
        .collect();

    (unmet, extra)
}

fn report_file(
    path: &Path,
    unmet: &[Expected],
    extra: &[Reported],
    allow_extra: bool,
) -> Option<String> {
    let extra: &[Reported] = if allow_extra { &[] } else { extra };
    if unmet.is_empty() && extra.is_empty() {
        return None;
    }

    let mut out = format!("{}:\n", path.display());
    for e in unmet {
        out += &format!(
            "  expected but not reported: line {} {} `{}`  (annotated on line {})\n",
            e.line, e.severity, e.needle, e.written_on
        );
    }
    for g in extra {
        out += &format!(
            "  reported but not expected: line {}:{} {}: {}\n",
            g.line, g.column, g.severity, g.message
        );
    }
    if !extra.is_empty() {
        out += "  to accept these, append to the offending line (chaining several \
                `//~` on one line is fine):\n";
        for g in extra {
            out += &format!(
                "    line {}:  //~ {} {}\n",
                g.line,
                g.severity.to_string().to_uppercase(),
                g.message
            );
        }
    }
    Some(out)
}

fn test_negative_file(path: &Path) {
    let filter = std::env::var("NEGATIVE_FILTER").ok();
    let mut failures: Vec<String> = Vec::new();
    let mut ignored: Vec<String> = Vec::new();
    let mut ran = 0usize;
    if let Some(filter) = &filter
        && !path.to_string_lossy().contains(filter.as_str())
    {
        return;
    }

    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    let mut try_test = || {
        let (directives, expectations) = match parse_annotations(&source) {
            Ok(parsed) => parsed,
            Err(e) => {
                failures.push(format!("{}:\n  bad annotation: {e}\n", path.display()));
                return;
            }
        };

        if let Some(reason) = directives.ignore {
            ignored.push(format!("{} ({reason})", path.display()));
            return;
        }
        ran += 1;

        let outcome =
            catch_unwind(AssertUnwindSafe(|| diagnostics_for(path, directives.no_std)));

        let reported = match outcome {
            Ok(Ok(reported)) => reported,
            Ok(Err(e)) => {
                failures.push(format!("{}:\n  {e}\n", path.display()));
                return;
            }
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_owned())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "<non-string panic payload>".to_owned());
                failures.push(format!(
                    "{}:\n  ICE: the compiler panicked instead of reporting a \
                     diagnostic:\n    {msg}\n",
                    path.display()
                ));
                return;
            }
        };

        if !reported.iter().any(|d| d.severity == Severity::Error) {
            failures.push(format!(
                "{}:\n  compiled without errors, but it lives in {FAIL_DIR}\n",
                path.display()
            ));
            return;
        }

        let (unmet, extra) = compare(&expectations, &reported);
        if let Some(report) = report_file(path, &unmet, &extra, directives.allow_extra) {
            failures.push(report);
        }
    };

    try_test();

    for note in &ignored {
        println!("ignored: {note}");
    }
    println!("negative: {ran} file(s) checked, {} ignored", ignored.len());

    assert!(
        failures.is_empty(),
        "\n{} negative test file(s) failed:\n\n{}",
        failures.len(),
        failures.join("\n")
    );
}

macro_rules! negative_test {
    ($fn_name:ident, $path:literal) => {
        #[test]
        fn $fn_name() {
            test_negative_file(&PathBuf::from($path));
        }
    };
}

mod neg {
    use super::*;

    negative_test!(assign, "examples/fail/assign.rkr");
    negative_test!(bad_type_defs, "examples/fail/bad_type_defs.rkr");
    negative_test!(r#match, "examples/fail/match.rkr");
    negative_test!(ret, "examples/fail/ret.rkr");
    negative_test!(unresolved, "examples/fail/unresolved.rkr");
}
