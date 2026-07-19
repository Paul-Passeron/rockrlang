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

fn run_snapshot_with_args(name: &str, folder: &str, args: &[&str]) {
    let example = format!("examples/{name}.rkr");
    let output = Command::new(env!("CARGO_BIN_EXE_rockrc"))
        .args([example.as_str()])
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run rockrc on {example}: {e}"));

    assert!(
        output.status.success(),
        "rockrc exited with an error on {example}:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let actual = String::from_utf8(output.stdout)
        .unwrap_or_else(|e| panic!("non-utf8 output for {example}: {e}"));

    let snapshot_path = Path::new("tests/snapshots/").join(format!("{name}.{folder}"));

    if std::env::var_os("ROCKR_UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(snapshot_path.parent().unwrap()).unwrap();
        std::fs::write(&snapshot_path, &actual).unwrap();
        return;
    }

    let expected = std::fs::read_to_string(&snapshot_path).unwrap_or_else(|e| {
        panic!(
            "missing snapshot {}: {e}\nrun with ROCKR_UPDATE_SNAPSHOTS=1 to create it",
            snapshot_path.display()
        )
    });

    assert_eq!(
        actual,
        expected,
        "Output for {example} changed.\nIf this is expected, re-run \
         with ROCKR_UPDATE_SNAPSHOTS=1 to update {}",
        snapshot_path.display()
    );
}

macro_rules! thir_snapshot_test {
    ($fn_name:ident, $example:literal) => {
        #[test]
        fn $fn_name() {
            run_snapshot_with_args($example, "thir", &["--skip-core", "--display-thir"]);
        }
    };
}

macro_rules! mir_snapshot_test {
    ($fn_name:ident, $example:literal) => {
        #[test]
        fn $fn_name() {
            run_snapshot_with_args($example, "mir", &["--skip-core", "--display-mir"]);
        }
    };
}

mod thir {
    use super::*;

    thir_snapshot_test!(auto_deref_field, "auto_deref_field");
    thir_snapshot_test!(destructure, "destructure");
    thir_snapshot_test!(empty, "empty");
    thir_snapshot_test!(generics, "generics");
    thir_snapshot_test!(hello, "hello");
    thir_snapshot_test!(if_stmt, "if");
    thir_snapshot_test!(impl_self, "impl_self");
    thir_snapshot_test!(liveness, "liveness");
    thir_snapshot_test!(loans, "loans");
    thir_snapshot_test!(nested_opt, "nested_opt");
    thir_snapshot_test!(printf, "printf");
    thir_snapshot_test!(ref_access, "ref_access");
    thir_snapshot_test!(share_mut, "share_mut");
    thir_snapshot_test!(simple_match, "simple_match");
    thir_snapshot_test!(while_loop, "while_loop");
    thir_snapshot_test!(implems, "implems");
    thir_snapshot_test!(zelf_non_receiver, "zelf_non_receiver");
    thir_snapshot_test!(typestates, "typestates");
    thir_snapshot_test!(pretty_print, "pretty_print");
    thir_snapshot_test!(match_structlit, "match_structlit");
    thir_snapshot_test!(supertraits, "supertraits");
    thir_snapshot_test!(receivers, "receivers");
    thir_snapshot_test!(for_loop, "for_loop");
}

mod mir {
    use super::*;

    mir_snapshot_test!(auto_deref_field, "auto_deref_field");
    mir_snapshot_test!(destructure, "destructure");
    mir_snapshot_test!(empty, "empty");
    mir_snapshot_test!(generics, "generics");
    mir_snapshot_test!(hello, "hello");
    mir_snapshot_test!(if_stmt, "if");
    mir_snapshot_test!(impl_self, "impl_self");
    mir_snapshot_test!(liveness, "liveness");
    mir_snapshot_test!(loans, "loans");
    mir_snapshot_test!(nested_opt, "nested_opt");
    mir_snapshot_test!(printf, "printf");
    mir_snapshot_test!(ref_access, "ref_access");
    mir_snapshot_test!(share_mut, "share_mut");
    mir_snapshot_test!(simple_match, "simple_match");
    mir_snapshot_test!(while_loop, "while_loop");
    mir_snapshot_test!(implems, "implems");
    mir_snapshot_test!(zelf_non_receiver, "zelf_non_receiver");
    mir_snapshot_test!(typestates, "typestates");
    mir_snapshot_test!(pretty_print, "pretty_print");
    mir_snapshot_test!(match_structlit, "match_structlit");
    mir_snapshot_test!(supertraits, "supertraits");
    mir_snapshot_test!(receivers, "receivers");
    mir_snapshot_test!(for_loop, "for_loop");
}
