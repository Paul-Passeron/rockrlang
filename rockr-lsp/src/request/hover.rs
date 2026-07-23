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

use crate::{Lsp, log};
use itertools::Itertools;
use lsp_types::{
    Hover, HoverContents, HoverParams, MarkedString, MarkupContent, MarkupKind,
};
use rockr::{
    common::{
        location::{Location, Span},
        symbols::Symbol,
    },
    lookup::{AstNode, ast_node_at, enclosing_fun, sig::SigNode, thir::ThirNode},
    name_resolve::{
        definition::Definition,
        type_expr::{get_templates_of_fun, struct_item, templates_of_struct},
    },
    parse_tree::top_level::AstTemplateArg,
    ril::{FunctionId, TypeDefId, TypeParamId, TypeRef},
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
        match ast_node_at(&self.db, loc)? {
            AstNode::ThirNode(thir, node) => {
                self.hover_thir(enclosing_fun(&self.db, loc)?, thir, node, loc)
            }
            AstNode::SigNode(node) => {
                Some(self.hover_sig(enclosing_fun(&self.db, loc)?, node))
            }
            AstNode::TypeNode(node) => {
                Some(self.hover_type_span_response(node.ty, node.span))
            }
            AstNode::Path(definition, span) => self.hover_path(definition, span),
        }
    }

    fn hover_type_span_response(&self, ty: TypeRef, span: Span) -> Hover {
        if let Some(struct_ref) = ty.as_struct_ref(&self.db)
            && let Some(blocks) = self
                .struct_display(
                    struct_ref.clone(),
                    StructDisplayOption::AllFields,
                    span.start(),
                )
                .map(|s| s.to_vec())
        {
            self.hover_span_response_blocks(blocks, span)
        } else {
            let ty_str =
                format!("`{}`", self.type_ref_to_named_string_at(span.start(), ty));
            self.hover_span_response(ty_str, span)
        }
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
            SigNode::ParamType(function_param) => {
                self.hover_type_span_response(function_param.ty, function_param.ty_span)
            }
            SigNode::ParamName(function_param) => {
                let name = function_param.name.to_string(&self.db);
                self.hover_span_response(
                    format!(
                        "arg {}: `{name}: {}\n`",
                        function_param.idx,
                        self.type_ref_to_named_string_at(
                            function_param.ty_span.start(),
                            function_param.ty
                        )
                    ),
                    function_param.name_span,
                )
            }
            SigNode::ReturnTy(return_ty) => {
                self.hover_type_span_response(return_ty.ty, return_ty.span)
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
                self.hover_function_infos(&func_ref, false, span)
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

    fn hover_thir(
        &self,
        func: FunctionId,
        thir: &Thir,
        node: ThirNode,
        loc: Location,
    ) -> Option<Hover> {
        match node {
            ThirNode::Expr { id, setup } => self.hover_expr(func, thir, id, setup),
            ThirNode::Place(idx) => Some(self.hover_place(func, thir, idx)),
            ThirNode::Local(idx) => Some(self.hover_local(func, thir, idx)),
            ThirNode::Stmt(stmt) => match &stmt.kind {
                StmtKind::Block {
                    semantic_infos: Some(BlockSemanticInfo::StructDestructure(struct_ref)),
                    ..
                } => {
                    let blocks = self
                        .struct_display(
                            struct_ref.clone(),
                            StructDisplayOption::AllFields,
                            loc,
                        )?
                        .to_vec();

                    Some(self.hover_span_response_blocks(blocks, stmt.span))
                }
                _ => None,
            },
            ThirNode::Pattern(pat) => {
                Some(self.hover_type_span_response(pat.ty, pat.span))
            }
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
                        loc,
                    )?
                    .to_vec();

                Some(self.hover_span_response_blocks(blocks, span))
            }
        }
    }

    fn hover_path(&self, definition: Definition, span: Span) -> Option<Hover> {
        if let Definition::Type(TypeDefId::Struct(struct_id)) = definition {
            let args = (0..templates_of_struct(&self.db, struct_id.interned()).len())
                .map(|i| TypeRef::Param(TypeParamId(i)))
                .collect();
            let display = self.struct_display(
                StructRef { def: struct_id, args },
                StructDisplayOption::AllFields,
                span.start(),
            )?;
            Some(self.hover_span_response_blocks(vec![display.struct_def], span))
        } else {
            Some(self.hover_span_response(
                format!("`{}`", definition.to_string(&self.db)),
                span,
            ))
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
                self.type_ref_to_named_string_at(func.span(&self.db).start(), local.ty)
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
        self.hover_type_span_response(place.ty, place.span)
    }

    fn hover_function_infos(
        &self,
        target_func: &FunctionRef,
        show_templates: bool,
        span: Span,
    ) -> Hover {
        let function_name = self.function_id_to_named_string(target_func.id);
        if !show_templates || target_func.args.is_empty() {
            return self.hover_span_response(format!("`{function_name}`"), span);
        }
        let templates = get_templates_of_fun(&self.db, target_func.id.interned());
        let template_string =
            self.template_substitutions(span.start(), &templates, &target_func.args);

        self.hover_span_response_blocks(
            vec![format!("```rockr\n{function_name}\n```"), template_string],
            span,
        )
    }

    fn template_substitutions(
        &self,
        loc: Location,
        templates: &[AstTemplateArg],
        args: &[TypeRef],
    ) -> String {
        templates
            .iter()
            .zip(args)
            .map(|(temp, arg)| {
                format!(
                    "`{}` = `{}`",
                    temp.name.to_string(&self.db),
                    self.type_ref_to_named_string_at(loc, *arg)
                )
            })
            .join(", ")
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
                Some(self.hover_function_infos(called, true, expr.span))
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
                Some(self.hover_type_span_response(expr.ty, expr.span))
            }
            ExprKind::StructLit { struct_def, .. } => {
                let blocks = self
                    .struct_display(
                        struct_def.clone(),
                        StructDisplayOption::AllFields,
                        expr.span.start(),
                    )?
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
        loc: Location,
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
                self.type_ref_to_named_string(&template_names, ty)
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

        let templates = (!template_defs.is_empty())
            .then(|| self.template_substitutions(loc, &template_defs, &struct_def.args));

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
