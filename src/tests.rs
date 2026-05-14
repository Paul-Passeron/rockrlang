#[cfg(test)]
mod tests {

    use std::path::PathBuf;

    use crate::{
        CompilerConfig, RockrDb, check_module_tree,
        driver::load_package,
        name_resolve::{core_package, std_package},
        print_module_tree,
    };

    fn parse_path(path: &str) -> bool {
        let db = RockrDb::new(CompilerConfig { no_std: false });
        let root_path = PathBuf::from(path);
        let package = load_package(&db, &root_path)
            .ok_or_else(|| format!("No package found at `{}`", root_path.display()))
            .unwrap();

        let mut packages = vec![package, core_package(&db)];
        if !db.config.no_std {
            packages.push(std_package(&db).unwrap());
        }
        println!("Package structure:");
        print_module_tree(&db, package.root(&db), 0);
        println!();

        let has_errors = check_module_tree(&db, package.root(&db), None, package, packages);

        return !has_errors;
    }

    #[test]
    fn std_io() {
        assert!(dbg!(parse_path("std/io")));
    }

    #[test]
    fn example_module_crate() {
        assert!(dbg!(parse_path("examples/module")));
    }

    #[test]
    fn example_args() {
        assert!(dbg!(parse_path("examples/args.rkr")));
    }

    #[test]
    fn example_empty() {
        assert!(dbg!(parse_path("examples/empty.rkr")));
    }

    #[test]
    fn example_hello_world() {
        assert!(dbg!(parse_path("examples/hello_world.rkr")));
    }

    #[test]
    fn example_index() {
        assert!(dbg!(parse_path("examples/index.rkr")));
    }

    #[test]
    fn example_modules() {
        assert!(dbg!(parse_path("examples/modules.rkr")));
    }

    #[test]
    fn example_smart_pointers() {
        assert!(dbg!(parse_path("examples/smart_pointers.rkr")));
    }

    #[test]
    fn example_slices() {
        assert!(dbg!(parse_path("examples/slices.rkr")));
    }

    #[test]
    fn example_tuples() {
        assert!(dbg!(parse_path("examples/tuples.rkr")));
    }
}
