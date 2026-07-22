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

use lsp_types::{GotoDefinitionParams, GotoDefinitionResponse};
use rockr::{
    common::location::{Location, Span},
    hir::{function_ast, hir_body, impl_sources},
    lookup::{AstNode, ast_node_at, enclosing_fun, sig::SigNode, thir::ThirNode},
    name_resolve::{
        definition::Definition,
        interfaces::interface_item,
        module_items,
        type_expr::{enum_item, get_templates_of_fun, struct_item},
    },
    parse_tree::top_level::AstTopLevelItemDesc,
    ril::{FunctionId, InterfaceId, ScopeOwnerId, TypeDefId, TypeRef},
    thir::{ExprId, ExprKind, LocalId, PlaceId, Thir},
};

use crate::{Lsp, log};

impl<'a> Lsp<'a> {
    pub fn handle_goto_def(
        &mut self,
        params: GotoDefinitionParams,
    ) -> Option<GotoDefinitionResponse> {
        let pos_params = params.text_document_position_params;
        let uri = pos_params.text_document.uri;
        let pos = pos_params.position;
        let loc = self.rockr_loc(uri, pos)?;
        if let Some(id) = enclosing_fun(&self.db, loc) {
            self.handle_goto_def_in_function(id, loc)
        } else {
            log!(
                "TODO: handle hover request at {} (Not inside a function)",
                loc.loc_info(&self.db)
            );
            None
        }
    }

    fn handle_goto_def_in_function(
        &self,
        func: FunctionId,
        loc: Location,
    ) -> Option<GotoDefinitionResponse> {
        let node = ast_node_at(&self.db, loc)?;
        match node {
            AstNode::ThirNode(thir, thir_node) => match thir_node {
                ThirNode::Expr { id, .. } => self.goto_def_expr(thir, id),
                ThirNode::Place(idx) => self.goto_def_place(thir, idx),
                ThirNode::Local(idx) => self.goto_def_local(thir, idx),
                ThirNode::Pattern(thir_pattern) => {
                    self.goto_def_ty_in_func(func, thir_pattern.ty, loc)
                }
                _ => None,
            },
            AstNode::SigNode(sig_node) => match sig_node {
                SigNode::ParamType(p) => self.goto_def_ty_in_func(func, p.ty, loc),
                SigNode::ParamName(_) => None,
                SigNode::ReturnTy(return_ty) => {
                    self.goto_def_ty_in_func(func, return_ty.ty, loc)
                }
                SigNode::TemplateParam { .. } => None,
                SigNode::FunctionName { .. } => None,
                SigNode::TemplateConstraint { constraint, .. } => {
                    Some(self.goto_def_interface(constraint.iref.def(&self.db)))
                }
            },
            AstNode::TypeNode(type_node) => {
                self.goto_def_ty_in_func(func, type_node.ty, loc)
            }
            AstNode::Path(definition, _) => {
                let def_span: Span = match definition {
                    Definition::Function(function_id) => Some(function_id.span(&self.db)),
                    Definition::Interface(interface_id) => {
                        Some(interface_item(&self.db, interface_id.into()).span)
                    }
                    Definition::Module(module_id) => {
                        module_id.parent(&self.db).map_or_else(
                            || {
                                module_id.package(&self.db).and_then(|pkg| {
                                    let sf = *pkg.root(&self.db).file(&self.db);
                                    Some(Span::new(sf, 0, sf.content(&self.db).len()))
                                })
                            },
                            |m| {
                                let items = module_items(&self.db, m.into()).as_ref()?;
                                items.iter().find_map(|item| match &item.data {
                                    AstTopLevelItemDesc::Module(spanned) => {
                                        if spanned.data.name.data
                                            == module_id.name(&self.db)
                                        {
                                            Some(spanned.span)
                                        } else {
                                            None
                                        }
                                    }
                                    _ => None,
                                })
                            },
                        )
                    }
                    Definition::Type(def) => match def {
                        TypeDefId::Builtin(_) => None,
                        TypeDefId::Struct(struct_id) => {
                            Some(struct_item(&self.db, struct_id.interned()).span)
                        }
                        TypeDefId::Enum(enum_id) => {
                            Some(enum_item(&self.db, enum_id.interned()).span)
                        }
                    },
                }?;
                Some(self.goto_span(def_span))
            }
        }
    }

    fn goto_def_ty_in_func(
        &self,
        func: FunctionId,
        ty: TypeRef,
        loc: Location,
    ) -> Option<GotoDefinitionResponse> {
        match ty {
            TypeRef::Concrete(type_id) => match type_id.def(&self.db) {
                TypeDefId::Builtin(_) => None,
                TypeDefId::Struct(struct_id) => {
                    let item = struct_item(&self.db, struct_id.into());
                    Some(self.goto_span(item.span))
                }
                TypeDefId::Enum(enum_id) => {
                    let item = enum_item(&self.db, enum_id.into());
                    Some(self.goto_span(item.span))
                }
            },
            TypeRef::Param(id) => {
                let id = id.0;
                let ts = get_templates_of_fun(&self.db, func.into());
                let ast = ts.get(id)?;
                Some(self.goto_span(ast.span))
            }
            TypeRef::Zelf => match func.parent(&self.db) {
                ScopeOwnerId::Module(_) => None,
                ScopeOwnerId::Impl(impl_id) => {
                    let src = impl_sources(&self.db, impl_id.into())
                        .iter()
                        .find(|src| src.span(&self.db).encloses(loc))?;
                    Some(self.goto_span(*src.span(&self.db)))
                }
                ScopeOwnerId::Interface(interface_ref) => {
                    Some(self.goto_def_interface(interface_ref.def(&self.db)))
                }
            },
            // TODO
            TypeRef::Associated(_) => None,
            _ => None,
        }
    }

    fn goto_def_interface(&self, interface: InterfaceId) -> GotoDefinitionResponse {
        let span = interface_item(&self.db, interface.into()).name.span;
        self.goto_span(span)
    }

    fn goto_def_local(
        &self,
        thir: &Thir,
        local: LocalId,
    ) -> Option<GotoDefinitionResponse> {
        let infos = &thir.locals[local];
        let src = infos.source?.0;
        // FIXME: Wtf ?
        let hir = hir_body(&self.db, thir.id)?;
        let local_infos = hir.locals(&self.db).iter().find(|infos| infos.id == src)?;
        let range = self.span(local_infos.span);
        let uri = self.uri_of_sf(local_infos.span.start().file);
        let location = lsp_types::Location::new(uri, range);
        Some(GotoDefinitionResponse::Scalar(location))
    }

    fn goto_def_place(
        &self,
        thir: &Thir,
        place: PlaceId,
    ) -> Option<GotoDefinitionResponse> {
        let base = match thir.places[place].base {
            rockr::thir::PlaceBase::Local(idx) => idx,
        };
        self.goto_def_local(thir, base)
    }

    fn goto_def_expr(&self, thir: &Thir, expr: ExprId) -> Option<GotoDefinitionResponse> {
        let e = &thir.exprs[expr];
        match &e.kind {
            ExprKind::Use(idx) => self.goto_def_place(thir, *idx),
            ExprKind::Call { called, .. } => {
                let ast = function_ast(&self.db, called.id.into()).inner(&self.db);
                Some(self.goto_span(ast.get_span()))
            }
            ExprKind::StructLit { struct_def, .. } => self.goto_def_ty_in_func(
                thir.id,
                struct_def.clone().as_type_ref(&self.db),
                e.span.start(),
            ),
            ExprKind::Constructor { enum_def, .. } => self.goto_def_ty_in_func(
                thir.id,
                enum_def.clone().as_type_ref(&self.db),
                e.span.start(),
            ),
            _ => None,
        }
    }

    fn goto_span(&self, span: Span) -> GotoDefinitionResponse {
        let range = self.span(span);
        let uri = self.uri_of_sf(span.file);
        let location = lsp_types::Location::new(uri, range);
        GotoDefinitionResponse::Scalar(location)
    }
}
