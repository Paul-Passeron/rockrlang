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
use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, DidChangeConfigurationParams,
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, Hover, HoverParams,
    HoverProviderCapability, InitializeParams, OneOf, Position, PublishDiagnosticsParams,
    Range, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
    notification::{
        DidChangeConfiguration, DidChangeTextDocument, DidOpenTextDocument,
        Notification as INotification, PublishDiagnostics,
    },
    request::{HoverRequest, Request as IRequest},
};
use rockr::{
    RockrDb, SourceFile,
    common::location::{Location, Span, offset_at},
    compiler::{
        Config, Workspace, compute_all_files_from_roots, compute_package_roots,
        diagnostic::{Diag, Severity},
        program_has_errors,
    },
    hir::{FunctionLikeAst, function_ast},
    lookup::{enclosing_fun, thir::ThirNode},
    ril::FunctionId,
    thir::{
        stmt::{BlockSemanticInfo, StmtKind},
        thir_body,
    },
};
use salsa::Setter;
use std::{
    collections::HashMap,
    fmt::Display,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    time::SystemTime,
};

fn log(msg: impl AsRef<str>) {
    let mut f =
        OpenOptions::new().create(true).append(true).open("/tmp/rockr-lsp.log").unwrap();
    let _ = writeln!(f, "{}", msg.as_ref());
}

macro_rules! log {
    ($($arg:tt)*) => {
        log(format!($($arg)*))
    };
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

fn main() -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::FULL,
        )),
        definition_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        references_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
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
        match notif.method.as_str() {
            DidOpenTextDocument::METHOD => {
                self.dispatch_notification::<DidOpenTextDocument>(notif, Self::did_open)
            }
            DidChangeTextDocument::METHOD => self
                .dispatch_notification::<DidChangeTextDocument>(notif, Self::did_change),
            DidChangeConfiguration::METHOD => self
                .dispatch_notification::<DidChangeConfiguration>(
                    notif,
                    Self::handle_did_change_configuration,
                ),
            method => log!("Unhandled notification method {method}"),
        }
    }

    fn loc(&self, loc: Location) -> Position {
        let infos = loc.loc_info(&self.db);
        Position::new(infos.line as u32 - 1, infos.column as u32 - 1)
    }

    fn span(&self, span: Span) -> Range {
        Range::new(self.loc(span.start()), self.loc(span.end()))
    }

    fn sev(&self, severity: Severity) -> DiagnosticSeverity {
        match severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Note => DiagnosticSeverity::INFORMATION,
            Severity::Help => DiagnosticSeverity::HINT,
        }
    }

    fn diag(&self, diag: &Diag) -> Diagnostic {
        Diagnostic::new(
            self.span(diag.primary.span),
            Some(self.sev(diag.severity)),
            None,
            diag.primary.message.clone(),
            diag.message.clone(),
            None,
            None,
        )
    }

    fn recheck(&self) {
        let (_, diags) = program_has_errors(&self.db);
        let mut diags_per_file: HashMap<SourceFile, Vec<&Diag>> = HashMap::new();
        for diag in diags {
            diags_per_file.entry(diag.primary.span.file).or_default().push(diag);
        }
        for file in self.db.files.iter() {
            let diags = diags_per_file.remove(file.value()).unwrap_or_default();
            let uri = self.uri_of_path(file.path(&self.db));
            let lsp_diags = diags.iter().map(|diag| self.diag(diag)).collect_vec();
            self.send_diagnostics(uri, lsp_diags);
        }
    }

    fn send_diagnostics(&self, uri: Uri, lsp_diags: Vec<Diagnostic>) {
        let n = Notification::new(
            PublishDiagnostics::METHOD.into(),
            PublishDiagnosticsParams::new(uri, lsp_diags, None),
        );
        self.connection.sender.send(Message::Notification(n)).unwrap();
    }

    fn path_of_uri(&self, uri: Uri) -> Option<PathBuf> {
        PathBuf::from(uri.path().as_str()).canonicalize().ok()
    }

    fn uri_of_path(&self, p: impl AsRef<Path>) -> Uri {
        Uri::from_str(format!("file://{}", p.as_ref().display()).as_str()).unwrap()
    }

    fn sf_of_uri(&self, uri: Uri) -> Option<SourceFile> {
        let path = self.path_of_uri(uri)?;
        let sf = *self.db.files.get(&path)?.value();
        Some(sf)
    }

    fn dispatch_notification<N: INotification>(
        &mut self,
        n: Notification,
        mut handler: impl FnMut(&mut Self, N::Params),
    ) {
        if n.method != N::METHOD {
            panic!(
                "Tried to handle notification method `{}` using `{}` handler",
                n.method,
                N::METHOD
            )
        }
        let parsed = serde_json::from_value::<N::Params>(n.params).unwrap();
        handler(self, parsed);
    }

    fn dispatch_request<R: IRequest, Err: Display>(
        &mut self,
        r: Request,
        mut handler: impl FnMut(&mut Self, R::Params) -> Result<R::Result, Err>,
    ) {
        if r.method != R::METHOD {
            panic!(
                "Tried to handle request method `{}` using `{}` handler",
                r.method,
                R::METHOD
            )
        }
        let parsed = serde_json::from_value::<R::Params>(r.params).unwrap();
        let response = match handler(self, parsed) {
            Ok(result) => Response::new_ok(r.id, result),
            Err(msg) => Response::new_err(r.id, 1, msg.to_string()),
        };
        self.connection.sender.send(Message::Response(response)).unwrap();
    }

    fn did_open(&mut self, params: DidOpenTextDocumentParams) {
        let path = self.path_of_uri(params.text_document.uri).unwrap();
        if !self.db.files.contains_key(&path) {
            compute_package_roots(&mut self.db, path)
                .unwrap_or_else(|err| panic!("{err}"));
            compute_all_files_from_roots(&mut self.db)
                .unwrap_or_else(|err| panic!("{err}"));
        }
        time(Some("recheck didOpen"), || self.recheck());
    }

    fn did_change(&mut self, did_change: DidChangeTextDocumentParams) {
        if did_change.content_changes.is_empty() {
            return;
        }
        let sf = self.sf_of_uri(did_change.text_document.uri).unwrap();
        sf.set_content(&mut self.db)
            .to(did_change.content_changes[0].text.as_str().into());
        time(Some("recheck didChange"), || self.recheck());
    }

    fn handle_did_change_configuration(&mut self, _: DidChangeConfigurationParams) {
        log!("TODO: handle didChangeConfiguration")
    }

    fn handle_request(&mut self, req: Request) {
        match req.method.as_str() {
            HoverRequest::METHOD => self
                .dispatch_request::<HoverRequest, String>(req, |this, params| {
                    Ok(this.handle_hover(params))
                }),
            _ => log!("TODO: Unhandled request ! {}", req.method),
        }
    }

    fn rockr_loc(&self, uri: Uri, pos: Position) -> Option<Location> {
        let sf = self.sf_of_uri(uri)?;
        let offset =
            offset_at(&self.db, sf, pos.line as usize + 1, pos.character as usize + 1);
        Some(Location { file: sf, offset })
    }

    fn handle_hover(&mut self, params: HoverParams) -> Option<Hover> {
        let pos_params = params.text_document_position_params;
        let uri = pos_params.text_document.uri;
        let pos = pos_params.position;
        let loc = self.rockr_loc(uri, pos)?;
        if let Some(id) = enclosing_fun(&self.db, loc) {
            self.handle_hover_in_function(id, loc)
        } else {
            log!(
                "TODO: handle hover request at {} (Not inside a function)",
                loc.loc_info(&self.db)
            );
            None
        }
    }

    fn handle_hover_in_function(
        &mut self,
        func: FunctionId,
        loc: Location,
    ) -> Option<Hover> {
        let db = &self.db;
        let body_span = match function_ast(db, func.into()).inner(db) {
            FunctionLikeAst::ExternDef(_, _) => None,
            FunctionLikeAst::Fundef(ast) => Some(ast.data.body_span),
            FunctionLikeAst::Method(ast) => Some(ast.data.body_span),
            FunctionLikeAst::TraitMethod(_) => None,
        }?;
        if !body_span.encloses(loc) {
            if loc.offset >= body_span.start_offset {
                return None;
            }
            log!(
                "TODO: handle hover request at {} (Inside function signature)",
                loc.loc_info(&self.db)
            );
            return None;
        }
        let thir_body = thir_body(db, func)?;
        let node = thir_body.node_at(&self.db, loc)?;
        log!("Found a node !");
        match node {
            ThirNode::Expr { id, setup } => {
                log!("Expr with id = {:?}, setup = {}", id.into_raw(), setup.is_some())
            }
            ThirNode::Place(idx) => log!("Place with id = {}", idx.into_raw()),
            ThirNode::Local(idx) => log!("Local with id = {}", idx.into_raw()),
            ThirNode::Stmt(stmt) => match &stmt.kind {
                StmtKind::Block {
                    semantic_infos: Some(BlockSemanticInfo::StructDestructure(struct_ref)),
                    ..
                } => log!(
                    "{}: Struct destructuring: {}",
                    stmt.span.start().loc_info(&self.db),
                    struct_ref.clone().as_type_ref(&self.db).to_string(&self.db)
                ),
                StmtKind::Block {
                    semantic_infos: Some(BlockSemanticInfo::ForLoop),
                    ..
                } => log!(
                    "{}: for-loop",
                    stmt.span.start().loc_info(&self.db),
                ),
                _ => log!("Stmt: {}", stmt.span.start().loc_info(&self.db)),
            },
            ThirNode::Pattern(pat) => {
                log!("Pattern: {}", pat.span.start().loc_info(&self.db))
            }
            ThirNode::MatchBranch(br) => {
                log!(
                    "Match branch: {}",
                    br.get_whole_span(thir_body).start().loc_info(&self.db)
                )
            }
        }
        None
    }
}

fn time<T, S: Display>(name: Option<S>, mut f: impl FnMut() -> T) -> T {
    let prefix = name.map_or("operation".into(), |x| format!("`{x}`"));
    let start = SystemTime::now();
    log!("Starting operation {prefix}");
    let res = f();
    let elapsed = start.elapsed().unwrap();
    log!("{prefix} took {} ms", elapsed.as_secs_f64() * 1000f64);
    res
}
