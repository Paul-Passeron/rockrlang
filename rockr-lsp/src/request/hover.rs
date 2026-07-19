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
    Lsp, log,
    naming::{function_id_to_named_string, type_ref_to_named_string_in},
};
use itertools::Itertools;
use lsp_types::{Hover, HoverContents, HoverParams, MarkedString};
use rockr::{
    common::location::{Location, Span},
    hir::{FunctionLikeAst, function_ast},
    lookup::{enclosing_fun, thir::ThirNode},
    name_resolve::type_expr::get_templates_of_fun,
    ril::{FunctionId, TypeRef},
    thir::{
        ExprId, ExprKind, LocalId, PlaceId, Thir,
        stmt::{BlockSemanticInfo, StmtKind, ThirStmt},
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

    fn hover_type_span_response(
        &self,
        func: FunctionId,
        ty: TypeRef,
        span: Span,
    ) -> Hover {
        let ty_str = format!("`{}`", type_ref_to_named_string_in(&self.db, func, ty));
        self.hover_span_response(ty_str, span)
    }

    fn hover_span_response(&self, md: String, span: Span) -> Hover {
        let range = self.span(span);
        let contents = HoverContents::Scalar(MarkedString::from_markdown(md));
        Hover { contents, range: Some(range) }
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
        let thir = thir_body(db, func)?;
        let node = thir.node_at(&self.db, loc)?;
        match node {
            ThirNode::Expr { id, setup } => self.hover_expr(func, thir, id, setup),
            ThirNode::Place(idx) => Some(self.hover_place(func, thir, idx)),
            ThirNode::Local(idx) => Some(self.hover_local(func, thir, idx)),
            ThirNode::Stmt(stmt) => match &stmt.kind {
                StmtKind::Block {
                    semantic_infos: Some(BlockSemanticInfo::StructDestructure(struct_ref)),
                    ..
                } => {
                    let ty = struct_ref.clone().as_type_ref(&self.db);
                    Some(self.hover_type_span_response(func, ty, stmt.span))
                }
                _ => {
                    // Nothing to say here
                    None
                }
            },
            ThirNode::Pattern(pat) => {
                Some(self.hover_type_span_response(func, pat.ty, pat.span))
            }
            // Nothing to say here
            ThirNode::MatchBranch(_) => None,
        }
    }

    fn hover_local(&self, func: FunctionId, thir: &Thir, idx: LocalId) -> Hover {
        let local = &thir.locals[idx];
        let local_name = if let Some((_, s)) = &local.source {
            s.to_string(&self.db)
        } else {
            format!("_{}", idx.into_raw())
        };
        self.hover_span_response(
            format!(
                "```rockr\nlet {}{local_name}: {}\n```",
                local.mutability,
                type_ref_to_named_string_in(&self.db, func, local.ty)
            ),
            local.span,
        )
    }

    fn hover_place(&self, func: FunctionId, thir: &Thir, idx: PlaceId) -> Hover {
        let place = &thir.places[idx];
        if place.projections.is_empty() {
            match place.base {
                rockr::thir::PlaceBase::Local(local) => {
                    let mut hover = self.hover_local(func, thir, local);
                    hover.range = Some(self.span(place.span));
                    return hover;
                }
            }
        }
        self.hover_type_span_response(func, place.ty, place.span)
    }

    fn hover_expr(
        &self,
        func: FunctionId,
        thir: &Thir,
        id: ExprId,
        _setup: Option<&[ThirStmt]>,
    ) -> Option<Hover> {
        let expr = &thir.exprs[id];
        match &expr.kind {
            ExprKind::Use(idx) => Some(self.hover_place(func, thir, *idx)),
            ExprKind::Call { called, .. } => {
                let function_name = function_id_to_named_string(&self.db, called.id);
                let hover_string = if called.args.is_empty() {
                    format!("`{function_name}`\n")
                } else {
                    let templates = get_templates_of_fun(&self.db, called.id.interned());
                    let template_string = templates
                        .iter()
                        .zip(&called.args)
                        .map(|(temp, arg)| {
                            format!(
                                "`{}` = `{}`",
                                temp.name.to_string(&self.db),
                                type_ref_to_named_string_in(&self.db, func, *arg)
                            )
                        })
                        .join(", ");

                    format!("`{function_name}`\n\n{template_string}\n")
                };
                Some(self.hover_span_response(hover_string, expr.span))
            }
            ExprKind::AddressOf { .. }
            | ExprKind::Ref { .. }
            | ExprKind::BinOp { .. }
            | ExprKind::StructLit { .. }
            | ExprKind::Neg(_)
            | ExprKind::Not(_)
            | ExprKind::Tuple(_)
            | ExprKind::SliceLit(_)
            | ExprKind::TypeName(_)
            | ExprKind::SizeOf(_)
            | ExprKind::Constructor { .. }
            | ExprKind::Metadata(_)
            | ExprKind::Cast(_, _) => {
                Some(self.hover_type_span_response(func, expr.ty, expr.span))
            }
            _ => None,
        }
    }
}
