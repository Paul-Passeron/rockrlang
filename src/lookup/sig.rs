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

use crate::{
    Db, SourceFile,
    common::{
        location::{Location, Span},
        symbols::Symbol,
    },
    hir::{FunctionLikeAst, function_ast},
    lookup::enclosing_fun,
    name_resolve::type_expr::templates_of_owner,
    parse_tree::{
        Spanned,
        top_level::{AstFundefArg, AstTemplateArg},
        type_expr::AstTypeExpr,
    },
    ril::{FunctionId, InterfaceRef, TypeRef},
    typecheck::inference::implicit::{AsAstImplCtx, AstImplicitContext},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionParam {
    pub idx: usize,
    pub name: Symbol,
    pub name_span: Span,
    pub ty: TypeRef,
    pub ty_span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReturnTy {
    pub ty: TypeRef,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TemplateParam {
    pub name: Symbol,
    pub idx: usize,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeConstraint {
    pub iref: InterfaceRef,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SigNode {
    ParamType(FunctionParam),
    ParamName(FunctionParam),
    ReturnTy(ReturnTy),
    TemplateParam { param: TemplateParam, constraints: Vec<Option<InterfaceRef>> },
    FunctionName { name: Symbol, span: Span },
    TemplateConstraint { param: TemplateParam, constraint: TypeConstraint },
}

struct GeneralSignature<'a> {
    pub name: &'a Spanned<Symbol>,
    pub ret_ty: &'a AstTypeExpr,
    pub args: &'a [AstFundefArg],
    pub templates: &'a [AstTemplateArg],
}

impl FunctionLikeAst {
    fn general_sig<'a>(&'a self) -> GeneralSignature<'a> {
        match self {
            FunctionLikeAst::ExternDef(ast, _) => GeneralSignature {
                name: &ast.data.name,
                ret_ty: &ast.data.return_type,
                args: &ast.data.args,
                templates: &ast.data.template_args,
            },
            FunctionLikeAst::Fundef(ast) => GeneralSignature {
                name: &ast.data.name,
                ret_ty: &ast.data.return_type,
                args: &ast.data.args,
                templates: &ast.data.template_args,
            },
            FunctionLikeAst::Method(ast) => GeneralSignature {
                name: &ast.data.name,
                ret_ty: &ast.data.return_type,
                args: &ast.data.args,
                templates: &ast.data.template_args,
            },
            FunctionLikeAst::TraitMethod(ast) => GeneralSignature {
                name: &ast.data.name,
                ret_ty: &ast.data.return_type,
                args: &ast.data.args,
                templates: &ast.data.template_args,
            },
        }
    }
}

impl FunctionId {
    pub fn body_span(self, db: &dyn Db) -> Option<Span> {
        match function_ast(db, self.into()).inner(db) {
            FunctionLikeAst::ExternDef(_, _) => None,
            FunctionLikeAst::Fundef(ast) => Some(ast.data.body_span),
            FunctionLikeAst::Method(ast) => Some(ast.data.body_span),
            FunctionLikeAst::TraitMethod(_) => None,
        }
    }
}

pub fn sig_node_at(db: &dyn Db, loc: Location) -> Option<SigNode> {
    #[salsa::tracked(returns(clone))]
    fn _aux(db: &dyn Db, sf: SourceFile, offset: usize) -> Option<SigNode> {
        let loc = Location::new(sf, offset);
        let func = enclosing_fun(db, loc)?;

        // Make sure we are inside the signature
        func.body_span(db)
            .is_none_or(|span| !span.encloses(loc) && loc.offset < span.start_offset)
            .then_some(())?;

        let ast = function_ast(db, func.into()).inner(db);
        general_sig_at(db, func, ast.general_sig(), loc)
    }
    _aux(db, loc.file, loc.offset)
}

fn general_sig_at(
    db: &dyn Db,
    func: FunctionId,
    sig: GeneralSignature<'_>,
    loc: Location,
) -> Option<SigNode> {
    if let Some(node) = fun_name_at(&sig.name, loc) {
        return Some(node);
    }

    let owner = func.parent(db);
    let ctx = AstImplicitContext::new(db, owner, sig.templates.iter().cloned().collect())
        .unwrap();

    if let Some(node) = ret_ty_at(db, &sig.ret_ty, &ctx, loc) {
        return Some(node);
    }

    let owner_template_offset = templates_of_owner(db, owner).len();
    for (i, templ) in sig.templates.iter().enumerate() {
        // Bail early in case of error (The `?`)
        if let Some(node) = templ_at(db, owner_template_offset + i, templ, &ctx, loc)? {
            return Some(node);
        }
    }

    for (i, arg) in sig.args.iter().enumerate() {
        if let Some(node) = arg_at(db, i, arg, &ctx, loc) {
            return Some(node);
        }
    }

    None
}

fn fun_name_at(name: &Spanned<Symbol>, loc: Location) -> Option<SigNode> {
    name.span.encloses(loc).then_some(())?;
    Some(SigNode::FunctionName { name: name.data, span: name.span })
}

fn ret_ty_at(
    db: &dyn Db,
    ret_ty: &AstTypeExpr,
    ctx: &AstImplicitContext,
    loc: Location,
) -> Option<SigNode> {
    ret_ty.span.encloses(loc).then_some(())?;
    let ty = ctx.resolve_err(db, &ret_ty.data);
    Some(SigNode::ReturnTy(ReturnTy { ty, span: ret_ty.span }))
}

fn arg_at(
    db: &dyn Db,
    idx: usize,
    arg: &AstFundefArg,
    ctx: &AstImplicitContext,
    loc: Location,
) -> Option<SigNode> {
    arg.span.encloses(loc).then_some(())?;
    let ty = ctx.resolve_err(db, &arg.ty.data);
    let param = FunctionParam {
        idx,
        name: arg.name,
        name_span: arg.name_span,
        ty,
        ty_span: arg.ty.span,
    };
    if arg.name_span.encloses(loc) {
        Some(SigNode::ParamName(param))
    } else if arg.ty.span.encloses(loc) {
        return Some(SigNode::ParamType(param));
    } else {
        None
    }
}

/// Returns `None` if an error has occured (Should drop current work)
/// Returns `Some(None)` if we didn't find anything (no error)
/// Returns `Some(Some(node))`s if it found something
fn templ_at(
    db: &dyn Db,
    idx: usize,
    templ: &AstTemplateArg,
    ctx: &AstImplicitContext,
    loc: Location,
) -> Option<Option<SigNode>> {
    let param = TemplateParam { name: templ.name, idx, span: templ.name_span };
    if !templ.span.encloses(loc) {
        return Some(None);
    }

    let resolved_constraints = templ
        .constraints
        .iter()
        .map(|constraint| ctx.resolve_interface(db, &constraint.data))
        .collect_vec();

    if templ.name_span.encloses(loc) {
        return Some(Some(SigNode::TemplateParam {
            param,
            constraints: resolved_constraints,
        }));
    }
    for constraint in &templ.constraints {
        if !constraint.span.encloses(loc) {
            return Some(None);
        }
        let Some(iref) = ctx.resolve_interface(db, &constraint.data) else {
            // Bail if we are in an unresolved interface.
            // This is an error !
            return None;
        };
        return Some(Some(SigNode::TemplateConstraint {
            param,
            constraint: TypeConstraint { iref, span: constraint.span },
        }));
    }
    return Some(None);
}
