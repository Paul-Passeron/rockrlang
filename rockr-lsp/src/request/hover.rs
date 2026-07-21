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

use std::convert::identity;

use crate::{
    Lsp, log,
    naming::{
        function_id_to_named_string, type_ref_to_named_string,
        type_ref_to_named_string_in,
    },
};
use itertools::Itertools;
use lsp_types::{
    Hover, HoverContents, HoverParams, MarkedString, MarkupContent, MarkupKind,
};
use rockr::{
    common::{
        location::{Location, Span},
        symbols::Symbol,
    },
    lookup::{
        FunctionNode, enclosing_fun, function_node_at, sig::SigNode, thir::ThirNode,
    },
    name_resolve::type_expr::{get_templates_of_fun, struct_item, templates_of_struct},
    ril::{FunctionId, TypeParamId, TypeRef},
    thir::{
        Dispatch, ExprId, ExprKind, FunctionRef, LocalId, PlaceId, StructRef, Thir,
        stmt::{BlockSemanticInfo, StmtKind, ThirStmt},
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

    fn hover_span_response_blocks(&self, mds: Vec<String>, span: Span) -> Hover {
        let value = mds.join("\n\n---\n\n");
        let contents =
            HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value });
        Hover { contents, range: Some(self.span(span)) }
    }

    fn hover_sig(&self, func: FunctionId, node: SigNode) -> Hover {
        match node {
            SigNode::ParamType(function_param) => self.hover_type_span_response(
                func,
                function_param.ty,
                function_param.ty_span,
            ),
            SigNode::ParamName(function_param) => {
                let name = function_param.name.to_string(&self.db);
                self.hover_span_response(
                    format!(
                        "arg {}: `{name}: {}\n`",
                        function_param.idx,
                        type_ref_to_named_string_in(&self.db, func, function_param.ty)
                    ),
                    function_param.name_span,
                )
            }
            SigNode::ReturnTy(return_ty) => {
                self.hover_type_span_response(func, return_ty.ty, return_ty.span)
            }
            SigNode::TemplateParam { param, constraints } => {
                let constraints_str = constraints
                    .into_iter()
                    .filter_map(identity)
                    .map(|cs| format!("`{}`", cs.to_string(&self.db)))
                    .join(" + ");

                self.hover_span_response(
                    format!(
                        "template {}: `{}`{}",
                        param.idx,
                        param.name.to_string(&self.db),
                        if constraints_str.is_empty() {
                            String::new()
                        } else {
                            format!(": {constraints_str}")
                        }
                    ),
                    param.span,
                )
            }
            SigNode::FunctionName { span, .. } => {
                let func_ref = FunctionRef {
                    id: func,
                    args: get_templates_of_fun(&self.db, func.into())
                        .iter()
                        .enumerate()
                        .map(|(i, _)| TypeRef::Param(TypeParamId(i)))
                        .collect(),
                    self_ty: Some(TypeRef::Zelf),
                    dispatch: Dispatch::Direct,
                };
                self.hover_function_infos(func, &func_ref, false, span)
            }
            SigNode::TemplateConstraint { param, constraint } => self
                .hover_span_response(
                    format!(
                        "template {}: `{}`: `{}`",
                        param.idx,
                        param.name.to_string(&self.db),
                        constraint.iref.to_string(&self.db)
                    ),
                    constraint.span,
                ),
        }
    }

    fn handle_hover_in_function(
        &mut self,
        func: FunctionId,
        loc: Location,
    ) -> Option<Hover> {
        let fun_node = function_node_at(&self.db, loc)?;
        match fun_node {
            FunctionNode::ThirNode(thir, node) => {
                match node {
                    ThirNode::Expr { id, setup } => {
                        self.hover_expr(func, thir, id, setup)
                    }
                    ThirNode::Place(idx) => Some(self.hover_place(func, thir, idx)),
                    ThirNode::Local(idx) => Some(self.hover_local(func, thir, idx)),
                    ThirNode::Stmt(stmt) => match &stmt.kind {
                        StmtKind::Block {
                            semantic_infos:
                                Some(BlockSemanticInfo::StructDestructure(struct_ref)),
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
                    ThirNode::EnumVariant { .. } => {
                        log!("TODO: handle enum variant");
                        None
                    }
                    ThirNode::StructField { struct_def, field, span } => {
                        let blocks = self
                            .struct_display(
                                struct_def,
                                StructDisplayOption::Fields(&[field]),
                                func,
                            )?
                            .to_vec();

                        Some(self.hover_span_response_blocks(blocks, span))
                    }
                }
            }
            FunctionNode::SigNode(node) => Some(self.hover_sig(func, node)),
            FunctionNode::TypeNode(node) => {
                Some(self.hover_type_span_response(func, node.ty, node.span))
            }
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
                "`let {}{local_name}: {}`",
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

    fn hover_function_infos(
        &self,
        in_func: FunctionId,
        target_func: &FunctionRef,
        show_templates: bool,
        span: Span,
    ) -> Hover {
        let function_name = function_id_to_named_string(&self.db, target_func.id);
        if !show_templates || target_func.args.is_empty() {
            return self.hover_span_response(format!("`{function_name}`"), span);
        }
        let templates = get_templates_of_fun(&self.db, target_func.id.interned());
        let template_string = templates
            .iter()
            .zip(&target_func.args)
            .map(|(temp, arg)| {
                format!(
                    "`{}` = `{}`",
                    temp.name.to_string(&self.db),
                    type_ref_to_named_string_in(&self.db, in_func, *arg)
                )
            })
            .join(", ");

        self.hover_span_response_blocks(
            vec![format!("```rockr\n{function_name}\n```"), template_string],
            span,
        )
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
                Some(self.hover_function_infos(func, called, true, expr.span))
            }
            ExprKind::AddressOf { .. }
            | ExprKind::Ref { .. }
            | ExprKind::BinOp { .. }
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
            ExprKind::StructLit { struct_def, .. } => {
                let blocks = self
                    .struct_display(struct_def.clone(), StructDisplayOption::AllFields, func)?
                    .to_vec();

                Some(self.hover_span_response_blocks(blocks, expr.span))
            }

            _ => None,
        }
    }

    fn struct_display(
        &self,
        struct_def: StructRef,
        opt: StructDisplayOption<'_>,
        func: FunctionId,
    ) -> Option<StructDisplay> {
        let struct_id = struct_def.def;
        let template_defs = templates_of_struct(&self.db, struct_id.interned());
        let template_names =
            template_defs.iter().map(|t| t.name.to_string(&self.db)).collect_vec();
        let struct_name = if template_defs.is_empty() {
            struct_id.name(&self.db).to_string(&self.db)
        } else {
            format!(
                "{}<{}>",
                struct_id.name(&self.db).to_string(&self.db),
                template_names.iter().join(", ")
            )
        };

        let field_str = |field: Symbol, ty| {
            format!(
                "    {}: {};\n",
                field.to_string(&self.db),
                type_ref_to_named_string(&self.db, &template_names, ty)
            )
        };

        let fields = match opt {
            StructDisplayOption::AllFields => {
                let tys = struct_def.get_fields_ty(&self.db);
                struct_item(&self.db, struct_def.def.into())
                    .fields
                    .iter()
                    .map(|field| field_str(field.name, tys[&field.name]))
                    .join("")
            }
            StructDisplayOption::Fields(symbols) => symbols
                .iter()
                .map(|field| {
                    Some(field_str(*field, struct_def.typeof_field(&self.db, *field)?))
                })
                .collect::<Option<Vec<_>>>()?
                .join(""),
        };

        let struct_def_extract =
            format!("```rockr\nstruct {struct_name} {{\n{fields}}}\n```",);

        let templates = (!template_defs.is_empty()).then(|| {
            template_defs
                .iter()
                .zip(&struct_def.args)
                .map(|(temp, arg)| {
                    format!(
                        "`{}` = `{}`",
                        temp.name.to_string(&self.db),
                        type_ref_to_named_string_in(&self.db, func, *arg)
                    )
                })
                .join(", ")
        });

        Some(StructDisplay { struct_def: struct_def_extract, templates })
    }
}

struct StructDisplay {
    struct_def: String,
    templates: Option<String>,
}

impl StructDisplay {
    pub fn to_vec(self) -> Vec<String> {
        match self.templates {
            Some(templates) => vec![self.struct_def, templates],
            None => vec![self.struct_def],
        }
    }
}

pub enum StructDisplayOption<'a> {
    AllFields,
    Fields(&'a [Symbol]),
}
