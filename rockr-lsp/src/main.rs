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

use itertools::Itertools;
use lsp_server::{Connection, Message, Notification, Request};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, InitializeParams, Position, PublishDiagnosticsParams,
    Range, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
    notification::{Notification as _, PublishDiagnostics},
};
use rockr::{
    RockrDb, SourceFile,
    check::check,
    compiler::{
        Config, Workspace, compute_all_files_from_roots, compute_package_roots,
        diagnostic::{Diag, Severity},
        program_has_errors,
    },
};
use salsa::Setter;
use std::{
    collections::HashMap, fmt::Display, fs::OpenOptions, io::Write, path::PathBuf,
    str::FromStr, time::SystemTime,
};

fn log(msg: impl AsRef<str>) {
    let mut f =
        OpenOptions::new().create(true).append(true).open("/tmp/rockr-lsp.log").unwrap();
    let _ = writeln!(f, "{}", msg.as_ref());
}
#[allow(unused)]
struct Lsp<'a> {
    pub db: RockrDb,
    pub connection: &'a Connection,
    pub params: InitializeParams,
}

impl<'a> Lsp<'a> {
    pub fn new(
        db: RockrDb,
        connection: &'a Connection,
        params: InitializeParams,
    ) -> Self {
        Self { db, connection, params }
    }
}

#[allow(unused)]
struct DidOpen<'a> {
    language_id: &'a str,
    text: &'a str,
    uri: &'a str,
    version: usize,
}

#[allow(unused)]
struct DidChange<'a> {
    content_changes: Vec<&'a str>,
    text_document_uri: &'a str,
    text_document_version: usize,
}

fn parse_did_open_notif(n: &Notification) -> Option<DidOpen<'_>> {
    if n.method != "textDocument/didOpen" {
        return None;
    }
    match &n.params {
        serde_json::Value::Object(map) => {
            let doc = map["textDocument"].as_object()?;
            let uri = doc["uri"].as_str()?;
            let language_id = doc["languageId"].as_str()?;
            let text = doc["text"].as_str()?;
            let version = doc["version"].as_i64()? as usize;
            Some(DidOpen { language_id, text, uri, version })
        }
        _ => None,
    }
}

fn parse_did_change(n: &Notification) -> Option<DidChange<'_>> {
    if n.method != "textDocument/didChange" {
        return None;
    }
    match &n.params {
        serde_json::Value::Object(map) => {
            let changes = map["contentChanges"]
                .as_array()?
                .iter()
                .map(|v| v.as_object()?["text"].as_str())
                .collect::<Option<Vec<_>>>()?;
            let doc = map["textDocument"].as_object()?;
            let uri = doc["uri"].as_str()?;
            let version = doc["version"].as_i64()? as usize;
            Some(DidChange {
                content_changes: changes,
                text_document_uri: uri,
                text_document_version: version,
            })
        }
        _ => None,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::FULL,
        )),
        ..Default::default()
    })?;

    let initialization_params = match connection.initialize(server_capabilities) {
        Ok(params) => params,
        Err(e) => {
            if e.channel_is_disconnected() {
                io_threads.join()?;
            }
            return Err(e.into());
        }
    };
    let params: InitializeParams = serde_json::from_value(initialization_params)?;

    let db = RockrDb::new();
    Workspace::initialize(
        &db,
        Config { no_std: false, skip_core: false, ..Default::default() },
    );

    let mut lsp = Lsp::new(db, &connection, params);
    lsp.main_loop()?;
    io_threads.join()?;
    Ok(())
}

impl<'a> Lsp<'a> {
    fn main_loop(&mut self) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
        for msg in &self.connection.receiver {
            match msg {
                Message::Request(req) => {
                    if self.connection.handle_shutdown(&req)? {
                        return Ok(());
                    }
                    self.handle_request(req);
                }
                Message::Response(_) => {}
                Message::Notification(notif) => {
                    self.handle_notif(notif);
                }
            }
        }
        Ok(())
    }

    fn handle_notif(&mut self, notif: Notification) {
        log(format!(
            "Trying to handle notif {}. Currently {} files in RockrDb.",
            notif.method,
            self.db.files.len()
        ));
        match notif.method.as_str() {
            "textDocument/didOpen" => self.handle_did_open(notif),
            "textDocument/didChange" => self.handle_did_change(notif),
            "workspace/didChangeConfiguration" => {
                self.handle_did_change_configuration(notif)
            }
            method => log(format!("Unhandled notification method {method}")),
        }
    }

    fn recheck(&mut self) {
        let now = SystemTime::now();
        let ws = Workspace::get(&self.db);
        log("Starting check...");
        check(&self.db, ws);
        log(format!(
            "Finished checking, took {} ms.",
            now.elapsed().unwrap().as_secs_f64() / 1000f64,
        ));
        let (_, diags) = program_has_errors(&self.db);
        let mut diags_per_file: HashMap<SourceFile, Vec<&Diag>> = HashMap::new();
        for diag in diags {
            diags_per_file.entry(diag.primary.span.file).or_default().push(diag);
        }
        for file in self.db.files.iter() {
            let file = *file.value();
            let diags = diags_per_file.remove(&file).unwrap_or_default();
            let uri = Uri::from_str(
                format!("file://{}", file.path(&self.db).display()).as_str(),
            )
            .unwrap();
            let lsp_diags = diags
                .iter()
                .map(|diag| {
                    let span = diag.primary.span;
                    let start_info = span.start().loc_info(&self.db);
                    let end_info = span.end().loc_info(&self.db);
                    Diagnostic::new(
                        Range::new(
                            Position::new(
                                start_info.line as u32 - 1,
                                start_info.column as u32 - 1,
                            ),
                            Position::new(
                                end_info.line as u32 - 1,
                                end_info.column as u32 - 1,
                            ),
                        ),
                        Some(match diag.severity {
                            Severity::Error => DiagnosticSeverity::ERROR,
                            Severity::Warning => DiagnosticSeverity::WARNING,
                            Severity::Note => DiagnosticSeverity::INFORMATION,
                            Severity::Help => DiagnosticSeverity::HINT,
                        }),
                        None,
                        diag.primary.message.clone(),
                        diag.message.clone(),
                        None,
                        None,
                    )
                })
                .collect_vec();
            let n = Notification::new(
                PublishDiagnostics::METHOD.into(),
                PublishDiagnosticsParams::new(uri, lsp_diags, None),
            );
            self.connection.sender.send(Message::Notification(n)).unwrap();
        }
    }

    fn handle_did_open(&mut self, notif: Notification) {
        let did_open = parse_did_open_notif(&notif).unwrap();
        let uri = Uri::from_str(did_open.uri).unwrap();
        let path = PathBuf::from(uri.path().as_str()).canonicalize().unwrap();
        if !self.db.files.contains_key(&path) {
            compute_package_roots(&mut self.db, path)
                .unwrap_or_else(|err| panic!("{err}"));
            compute_all_files_from_roots(&mut self.db)
                .unwrap_or_else(|err| panic!("{err}"));
        }
        time(Some("recheck didOpen"), || self.recheck());
    }

    fn handle_did_change(&mut self, notif: Notification) {
        let did_change = parse_did_change(&notif).unwrap();
        let uri = Uri::from_str(did_change.text_document_uri).unwrap();
        let path = PathBuf::from(uri.path().as_str()).canonicalize().unwrap();
        let sf = *self.db.files.get(&path).unwrap().value();
        if did_change.content_changes.is_empty() {
            return;
        }
        sf.set_content(&mut self.db).to(did_change.content_changes[0].into());
        time(Some("recheck didChange"), || self.recheck());
    }

    fn handle_did_change_configuration(&mut self, _: Notification) {
        log(format!("{}:{}: TODO: handle didChangeConfiguration", file!(), line!()))
    }

    fn handle_request(&mut self, _req: Request) {
        log("TOOD: Need to handle request !");
    }
}

fn time<T, S: Display>(name: Option<S>, mut f: impl FnMut() -> T) -> T {
    let prefix = name.map_or("operation".into(), |x| format!("`{x}`"));
    let start = SystemTime::now();
    log(format!("Starting operation {prefix}"));
    let res = f();
    let elapsed = start.elapsed().unwrap();
    log(format!("{prefix} took {} ms", elapsed.as_secs_f64() * 1000f64));
    res
}
