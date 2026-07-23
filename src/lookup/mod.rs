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
    Db, SourceFile,
    common::location::{Location, Span},
    compiler::{Workspace, workspace_packages},
    hir::{impl_items, interface_items},
    lookup::{
        path::path_node_at,
        sig::{SigNode, sig_node_at},
        thir::ThirNode,
        ty::{TypeNode, type_node_at},
    },
    name_resolve::{
        definition::Definition, file_module_id, implems::module_impls, module_items,
    },
    parse_tree::top_level::{AstImplItem, AstInterfaceItem, AstTopLevelItemDesc},
    ril::{
        FileModule, FunctionId, InterfaceId, InterfaceRef, ModuleId, Package,
        ScopeOwnerId, TypeParamId, TypeRef,
    },
    thir::{Thir, thir_body},
};

pub mod path;
pub mod sig;
pub mod thir;
pub mod ty;

fn file_module_of<'db>(
    db: &'db dyn Db,
    file: SourceFile,
) -> Option<(FileModule<'db>, Package<'db>, Option<ModuleId>)> {
    fn find<'db>(
        db: &'db dyn Db,
        pkg: Package<'db>,
        fm: FileModule<'db>,
        target: SourceFile,
        parent: Option<ModuleId>,
    ) -> Option<(FileModule<'db>, Option<ModuleId>)> {
        if *fm.file(db) == target {
            return Some((fm, parent));
        }
        let parent = file_module_id(db, fm, parent, pkg);
        fm.submodules(db).iter().find_map(|sub| find(db, pkg, *sub, target, Some(parent)))
    }
    workspace_packages(db, Workspace::get(db)).iter().find_map(|pkg| {
        find(db, *pkg, *pkg.root(db), file, None).map(|(fm, parent)| (fm, *pkg, parent))
    })
}

#[salsa::tracked(returns(copy))]
fn module_of_sf(db: &dyn Db, file: SourceFile) -> Option<ModuleId> {
    let (file_module, package, parent) = file_module_of(db, file)?;
    let module = file_module_id(db, file_module, parent, package);
    Some(module)
}

pub fn enclosing_scope_owner(db: &dyn Db, loc: Location) -> Option<ScopeOwnerId> {
    #[salsa::tracked(returns(copy))]
    fn _enclosing_scope_owner(
        db: &dyn Db,
        file: SourceFile,
        offset: usize,
    ) -> Option<ScopeOwnerId> {
        fn enclosing_owner_in_module(
            db: &dyn Db,
            loc: Location,
            module: ModuleId,
        ) -> Option<ScopeOwnerId> {
            module_items(db, module.into()).as_ref()?.iter().find_map(|item| {
                item.span.encloses(loc).then_some(())?;
                match &item.data {
                    AstTopLevelItemDesc::Module(ast) => {
                        let interned = module.interned();
                        let child = ModuleId::new(
                            db,
                            ast.data.name.data,
                            Some(module),
                            None,
                            vec![],
                            *interned.package(db),
                        );
                        enclosing_owner_in_module(db, loc, child)
                    }
                    AstTopLevelItemDesc::Interface(ast) => {
                        let iref = InterfaceRef::new(
                            db,
                            InterfaceId::new(db, ast.name.data, module),
                            (0..ast.template_args.len())
                                .map(|i| TypeRef::Param(TypeParamId(i)))
                                .collect(),
                        );
                        Some(ScopeOwnerId::Interface(iref))
                    }
                    AstTopLevelItemDesc::Impl(ast_impl_block) => {
                        module_impls(db, module.interned())
                            .iter()
                            .find(|src| *src.span(db) == ast_impl_block.span)
                            .map(|src| ScopeOwnerId::Impl(*src.id(db)))
                    }

                    AstTopLevelItemDesc::Error(_)
                    | AstTopLevelItemDesc::Fundef(_)
                    | AstTopLevelItemDesc::StructDef(_)
                    | AstTopLevelItemDesc::EnumDef(_)
                    | AstTopLevelItemDesc::ExternDef(_, _) => {
                        Some(ScopeOwnerId::Module(module))
                    }
                }
            })
        }
        let module = module_of_sf(db, file)?;
        let loc = Location::new(file, offset);
        enclosing_owner_in_module(db, loc, module)
    }
    _enclosing_scope_owner(db, loc.file, loc.offset)
}

pub fn enclosing_fun(db: &dyn Db, loc: Location) -> Option<FunctionId> {
    let scope_owner = enclosing_scope_owner(db, loc)?;
    match scope_owner {
        ScopeOwnerId::Module(module_id) => module_items(db, module_id.interned())
            .as_ref()?
            .iter()
            .find_map(|item| match &item.data {
                AstTopLevelItemDesc::Fundef(fdef) => {
                    fdef.span.encloses(loc).then_some(())?;
                    Some(FunctionId::new(db, fdef.data.name.data, scope_owner))
                }
                _ => None,
            }),
        ScopeOwnerId::Impl(impl_id) => {
            impl_items(db, impl_id.into()).iter().find_map(|item| match item {
                AstImplItem::Type { .. } => None,
                AstImplItem::Fundef(fdef) => {
                    fdef.span.encloses(loc).then_some(())?;
                    Some(FunctionId::new(db, fdef.data.name.data, scope_owner))
                }
            })
        }
        ScopeOwnerId::Interface(interface_ref) => {
            interface_items(db, interface_ref.def(db).into()).iter().find_map(|item| {
                match item {
                    AstInterfaceItem::Type { .. } => None,
                    AstInterfaceItem::Sig(fdef) => {
                        fdef.span.encloses(loc).then_some(())?;
                        Some(FunctionId::new(db, fdef.data.name.data, scope_owner))
                    }
                }
            })
        }
    }
}

pub enum AstNode<'a> {
    ThirNode(&'a Thir, ThirNode<'a>),
    SigNode(SigNode),
    TypeNode(TypeNode),
    Path(Definition, Span),
}

impl AstNode<'_> {
    pub fn span(&self) -> Span {
        match self {
            AstNode::ThirNode(thir, thir_node) => thir_node.span(thir),
            AstNode::SigNode(sig_node) => sig_node.span(),
            AstNode::TypeNode(type_node) => type_node.span,
            AstNode::Path(_, span) => *span,
        }
    }
}

pub fn ast_node_at(db: &dyn Db, loc: Location) -> Option<AstNode<'_>> {
    let path_node = path_node_at(db, loc).map(|(def, span)| AstNode::Path(def, span));

    let type_node = type_node_at(db, loc);

    let fun_node = enclosing_fun(db, loc).and_then(|f| {
        sig_node_at(db, loc).map(AstNode::SigNode).or_else(|| {
            let thir = thir_body(db, f)?;
            thir.node_at(db, loc).map(|node| AstNode::ThirNode(thir, node))
        })
    });

    let base = if let Some(type_node) = type_node
        && let Some(fun_node) = fun_node
    {
        if type_node.span == fun_node.span() {
            Some(fun_node)
        } else {
            Some(AstNode::TypeNode(type_node))
        }
    } else {
        type_node.map(AstNode::TypeNode).or(fun_node)
    };

    match (base, path_node) {
        (
            Some(AstNode::TypeNode(ty_node)),
            Some(AstNode::Path(Definition::Type(_), _)),
        ) => Some(AstNode::TypeNode(ty_node)),
        (Some(base), Some(path)) if path.span().len() <= base.span().len() => Some(path),
        (Some(base), _) => Some(base),
        (None, path) => path,
    }
}
