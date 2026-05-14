use nonempty::NonEmpty;

use crate::{CliArgs, SourceFile, driver::file_args::get_non_empty_files};

#[allow(unused_imports)]
pub use file_args::get_non_empty_files_from_paths;

mod file_args;

pub struct ParsedArgs<'db> {
    pub files: NonEmpty<SourceFile<'db>>,
}

pub fn parsed_args_from_cli<'db>(
    db: &'db dyn crate::Db,
    args: &CliArgs,
) -> Option<ParsedArgs<'db>> {
    let non_empty = get_non_empty_files(db, args.files.as_ref().map(Vec::as_slice))?;
    Some(ParsedArgs { files: non_empty })
}
