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

use crate::{Db, common::location::Span, name_resolve::definition::Definition};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Note => "note",
                Severity::Help => "help",
            }
        )
    }
}

#[salsa::accumulator]
#[derive(Debug, Clone)]
pub struct Diag {
    pub severity: Severity,
    pub message: String,
    pub primary: DiagLabel,
    pub secondary: Vec<DiagLabel>,
    pub notes: Vec<String>,
    pub help: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DiagLabel {
    pub span: Span,
    pub message: Option<String>,
}

impl Diag {
    pub fn redefinition(db: &dyn Db, defs: Vec<Definition>) -> Self {
        assert!(!defs.is_empty());
        let (first, others) = {
            let mut defs = defs;
            let head = defs.remove(0);
            (head, defs)
        };

        let primary = DiagLabel::new(
            first.name_span(db).unwrap(),
            Some(format!("Defined here")),
        );
        let secondary = others
            .into_iter()
            .map(|def| {
                DiagLabel::new(
                    def.name_span(db).unwrap(),
                    Some(format!("Defined here")),
                )
            })
            .collect();

        Diag::new(
            Severity::Error,
            format!(
                "name `{}` is defined multiple times at top-level.",
                first.name(db).to_string(db)
            ),
            primary,
            secondary,
            vec![],
            vec![],
        )
    }

    pub fn todo(message: String, span: Span) -> Self {
        let primary = DiagLabel::new(span, Some(message));

        Diag::new(
            Severity::Warning,
            "not yet implemented.".to_string(),
            primary,
            vec![],
            vec![],
            vec![],
        )
    }

    pub fn generic_error(message: String, span: Span) -> Self {
        let primary = DiagLabel::new(span, Some(message));

        Diag::new(
            Severity::Error,
            "not yet implemented.".to_string(),
            primary,
            vec![],
            vec![],
            vec![],
        )
    }

    pub fn new(
        severity: Severity,
        message: String,
        primary: DiagLabel,
        secondary: Vec<DiagLabel>,
        notes: Vec<String>,
        help: Vec<String>,
    ) -> Self {
        Self {
            severity,
            message,
            primary,
            secondary,
            notes,
            help,
        }
    }
}

impl DiagLabel {
    pub fn new(span: Span, message: Option<String>) -> Self {
        Self { span, message }
    }
}
