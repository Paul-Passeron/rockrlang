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

use crate::{Lsp, log};
use lsp_types::{Hover, HoverParams};
use rockr::{
    common::location::Location,
    hir::{FunctionLikeAst, function_ast},
    lookup::{enclosing_fun, thir::ThirNode},
    ril::FunctionId,
    thir::{
        stmt::{BlockSemanticInfo, StmtKind},
        thir_body,
    },
};

impl<'a> Lsp<'a> {
    pub(super) fn handle_hover(&mut self, params: HoverParams) -> Option<Hover> {
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
                } => log!("{}: for-loop", stmt.span.start().loc_info(&self.db),),
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
