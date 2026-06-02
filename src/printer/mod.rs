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

use std::{collections::HashMap, io};

use codespan_reporting::diagnostic::{
    Diagnostic as CrDiagnostic, Label as CrLabel, LabelStyle,
    Severity as CrSeverity,
};
use codespan_reporting::files::SimpleFiles;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
use codespan_reporting::term::{self, Config, termcolor::WriteColor};

use crate::compiler::diagnostic::{Diag, DiagLabel, Severity};
use crate::{Db, SourceFile};

pub mod type_printer;

pub fn render_diagnostics<'db, 'diag>(
    db: &'db dyn Db,
    diags: impl Iterator<Item = &'diag Diag>,
) {
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);
    _render_diagnostics(db, diags, &mut stderr).unwrap();
}

pub fn _render_diagnostics<'a, W: WriteColor>(
    db: &dyn Db,
    diags: impl Iterator<Item = &'a Diag>,
    out: &mut W,
) -> io::Result<()> {
    let mut files = SimpleFiles::new();
    let mut file_ids: HashMap<SourceFile, usize> = HashMap::new();

    let config = Config::default();

    for diag in diags {
        let cr = build_diagnostic(db, diag, &mut files, &mut file_ids);

        term::emit_to_write_style(out, &config, &files, &cr)
            .map_err(io_error)?;
    }

    Ok(())
}

fn build_diagnostic<'db>(
    db: &'db dyn Db,
    diag: &Diag,
    files: &mut SimpleFiles<String, &'db str>,
    file_ids: &mut HashMap<SourceFile, usize>,
) -> CrDiagnostic<usize> {
    let primary = label_for(
        db,
        diag.primary.clone(),
        LabelStyle::Primary,
        files,
        file_ids,
    );
    let secondary: Vec<_> = diag
        .secondary
        .iter()
        .map(|l| {
            label_for(db, l.clone(), LabelStyle::Secondary, files, file_ids)
        })
        .collect();

    let mut labels = Vec::with_capacity(1 + secondary.len());
    labels.push(primary);
    labels.extend(secondary);

    let mut notes = diag.notes.clone();
    notes.extend(diag.help.iter().map(|h| format!("help: {h}")));

    CrDiagnostic::new(severity_to_cr(diag.severity))
        .with_message(diag.message.clone())
        .with_labels(labels)
        .with_notes(notes)
}

fn label_for<'db>(
    db: &'db dyn Db,
    label: DiagLabel,
    style: LabelStyle,
    files: &mut SimpleFiles<String, &'db str>,
    file_ids: &mut HashMap<SourceFile, usize>,
) -> CrLabel<usize> {
    let span = label.span;
    let file_id = ensure_file(db, span.file, files, file_ids);
    let range = (span.start_offset as usize)..(span.end_offset as usize);

    let mut l = CrLabel::new(style, file_id, range);
    if let Some(msg) = label.message {
        l = l.with_message(msg);
    }
    l
}

fn ensure_file<'db>(
    db: &'db dyn Db,
    file: SourceFile,
    files: &mut SimpleFiles<String, &'db str>,
    file_ids: &mut HashMap<SourceFile, usize>,
) -> usize {
    if let Some(&id) = file_ids.get(&file) {
        return id;
    }
    let name = file.path(db).display().to_string();
    let text: &str = file.content(db);
    let id = files.add(name, text);
    file_ids.insert(file, id);
    id
}

fn severity_to_cr(s: Severity) -> CrSeverity {
    match s {
        Severity::Error => CrSeverity::Error,
        Severity::Warning => CrSeverity::Warning,
        Severity::Note => CrSeverity::Note,
        Severity::Help => CrSeverity::Help,
    }
}

fn io_error(e: codespan_reporting::files::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e)
}
