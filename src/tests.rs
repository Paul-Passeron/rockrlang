#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        RockrDb, SourceFile, SourceRoot,
        driver::get_non_empty_files_from_paths,
        parser::{ParseError, parse_file},
    };

    fn parse_path(path: &str) -> Vec<String> {
        let db = RockrDb::default();
        let files =
            get_non_empty_files_from_paths(&db, &[PathBuf::from(path)]).expect("no files found");
        let root = SourceRoot::new(&db, files.into());
        let mut errors = Vec::new();
        for file in root.files(&db) {
            let source = SourceFile::new(&db, file.file(&db));
            parse_file(&db, root, source);
            let errs: Vec<&ParseError> = parse_file::accumulated::<ParseError>(&db, root, source);
            for e in errs {
                errors.push(format!("{:?}", e));
            }
        }
        errors
    }

    #[test]
    fn std_io() {
        assert!(dbg!(parse_path("std/io")).is_empty());
    }

    #[test]
    fn example_module_crate() {
        assert!(dbg!(parse_path("examples/module")).is_empty());
    }

    #[test]
    fn example_args() {
        assert!(dbg!(parse_path("examples/args.rkr")).is_empty());
    }

    #[test]
    fn example_empty() {
        assert!(dbg!(parse_path("examples/empty.rkr")).is_empty());
    }

    #[test]
    fn example_hello_world() {
        assert!(dbg!(parse_path("examples/hello_world.rkr")).is_empty());
    }

    #[test]
    fn example_index() {
        assert!(dbg!(parse_path("examples/index.rkr")).is_empty());
    }

    #[test]
    fn example_modules() {
        assert!(dbg!(parse_path("examples/modules.rkr")).is_empty());
    }

    #[test]
    fn example_smart_pointers() {
        assert!(dbg!(parse_path("examples/smart_pointers.rkr")).is_empty());
    }

    #[test]
    fn example_slices() {
        assert!(dbg!(parse_path("examples/slices.rkr")).is_empty());
    }

    #[test]
    fn example_tuples() {
        assert!(dbg!(parse_path("examples/tuples.rkr")).is_empty());
    }
}
