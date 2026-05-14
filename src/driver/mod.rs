use nonempty::NonEmpty;

use crate::{CliArgs, SourceFileContent, driver::file_args::get_non_empty_files};

#[allow(unused_imports)]
pub use file_args::get_non_empty_files_from_paths;

mod file_args;

pub struct ParsedArgs {
    pub files: NonEmpty<SourceFileContent>,
}

pub fn parsed_args_from_cli(db: &dyn crate::Db, args: &CliArgs) -> Option<ParsedArgs> {
    let non_empty = get_non_empty_files(db, args.files.as_ref().map(Vec::as_slice))?;
    Some(ParsedArgs { files: non_empty })
}
