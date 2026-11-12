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

use crate::{
    Db,
    check::check,
    common::{diagnostic::Diag, load::load_workspace_from_disk, location::LocationInfo},
    printer::render_diagnostics,
    workspace::{Workspace, config::Config, error::CompilerError},
};
use std::path::PathBuf;

pub fn program_has_errors(db: &dyn Db) -> (bool, Vec<&Diag>) {
    let ws = Workspace::get(db);
    let raw_diags: Vec<&Diag> = check::accumulated::<Diag>(db, ws);
    let has_errors = raw_diags.iter().any(|d| d.is_error());
    (has_errors, raw_diags)
}

pub fn check_from_disk(root: PathBuf, config: Config) -> Result<(), CompilerError> {
    let db = load_workspace_from_disk(root, config)?;
    let ws = Workspace::get(&db);
    check(&db, ws);
    let (has_errors, raw_diags) = program_has_errors(&db);

    let mut diags: Vec<(&LocationInfo, &Diag)> = raw_diags
        .into_iter()
        .map(|d| (d.primary.span.start().loc_info(&db), d))
        .collect();

    diags.sort_by(|(loc_a, diag_a), (loc_b, diag_b)| {
        diag_a.severity.cmp(&diag_b.severity).then_with(|| {
            loc_a.cmp(loc_b).then_with(|| diag_a.message.cmp(&diag_b.message))
        })
    });

    render_diagnostics(&db, diags.into_iter().map(|(_, diag)| diag));
    if has_errors { Err(CompilerError::CompiledWithErrors) } else { Ok(()) }
}
