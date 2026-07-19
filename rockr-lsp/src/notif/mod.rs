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

use crate::{Lsp, log, time};
use lsp_server::Notification;
use lsp_types::{
    DidChangeConfigurationParams, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    notification::{
        DidChangeConfiguration, DidChangeTextDocument, DidOpenTextDocument,
        Notification as INotification,
    },
};
use rockr::compiler::{compute_all_files_from_roots, compute_package_roots};
use salsa::Setter;

impl<'a> Lsp<'a> {
    pub fn handle_notif(&mut self, notif: Notification) {
        match notif.method.as_str() {
            DidOpenTextDocument::METHOD => {
                self.dispatch_notification::<DidOpenTextDocument>(notif, Self::did_open)
            }
            DidChangeTextDocument::METHOD => self
                .dispatch_notification::<DidChangeTextDocument>(notif, Self::did_change),
            DidChangeConfiguration::METHOD => self
                .dispatch_notification::<DidChangeConfiguration>(
                    notif,
                    Self::did_change_configuration,
                ),
            method => log!("Unhandled notification method {method}"),
        }
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

    fn did_change_configuration(&mut self, _: DidChangeConfigurationParams) {
        log!("TODO: handle didChangeConfiguration")
    }
}
