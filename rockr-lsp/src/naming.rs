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
use rockr::{
    common::location::Location,
    hir::Mutability,
    hir::signature::{FunctionSignature, get_sig_of_function},
    printer::type_printer::TypePrinter,
    resolved::{
        BuiltinTypeId, BuiltinTypeKind, FunctionId, ImplId, InterfaceRef, ScopeOwnerId,
        TypeDefId, TypeId, TypeRef,
    },
};

use crate::Lsp;

impl<'a> Lsp<'a> {
    pub fn template_names(&self, loc: Location) -> Vec<String> {
        self.templates_at_loc(loc).iter().map(|t| t.name.to_string(&self.db)).collect()
    }

    pub fn type_ref_to_named_string_at(&self, loc: Location, ty: TypeRef) -> String {
        self.type_ref_to_named_string(&self.template_names(loc), ty)
    }

    fn param_name(&self, names: &[String], index: usize) -> String {
        names.get(index).cloned().unwrap_or_else(|| format!("T{index}"))
    }

    fn is_def_builtin(&self, def: TypeDefId) -> Option<BuiltinTypeId> {
        match def {
            TypeDefId::Builtin(id) => Some(id),
            _ => None,
        }
    }

    fn builtin_pretty_print(&self, def: BuiltinTypeId) -> Option<(String, String)> {
        match def.kind(&self.db) {
            BuiltinTypeKind::Ref { mutability } => {
                let muta = match mutability {
                    Mutability::Const => "",
                    Mutability::Mutable => "mut ",
                };
                Some((format!("&{muta}"), String::new()))
            }
            BuiltinTypeKind::Ptr { mutability } => {
                let muta = match mutability {
                    Mutability::Const => "",
                    Mutability::Mutable => "mut ",
                };
                Some((format!("*{muta}"), String::new()))
            }
            BuiltinTypeKind::Tuple => Some(("(".into(), ")".into())),
            BuiltinTypeKind::Slice => Some(("[".into(), "]".into())),
            _ => None,
        }
    }

    pub fn type_ref_to_named_string(&self, names: &[String], ty: TypeRef) -> String {
        match ty {
            TypeRef::Concrete(type_id) => self.type_id_to_named_string(names, type_id),
            TypeRef::Associated(symbol) => {
                format!("Self::{}", symbol.to_string(&self.db))
            }
            TypeRef::Param(type_param_id) => self.param_name(names, type_param_id.0),
            TypeRef::Zelf => "Self".to_string(),
            TypeRef::Error => "<ERROR>".to_string(),
            TypeRef::Unknown => "<???>".to_string(),
        }
    }

    pub fn type_id_to_named_string(&self, names: &[String], type_id: TypeId) -> String {
        let def = type_id.def(&self.db);
        let args = type_id.args(&self.db);
        if args.is_empty() {
            return TypePrinter::new().type_def_id_to_string(&self.db, def);
        }
        let args_str =
            args.iter().map(|ty| self.type_ref_to_named_string(names, *ty)).join(", ");
        if let Some(id) = self.is_def_builtin(def)
            && let Some((prefix, suffix)) = self.builtin_pretty_print(id)
        {
            format!("{prefix}{args_str}{suffix}")
        } else {
            format!(
                "{}<{args_str}>",
                TypePrinter::new().type_def_id_to_string(&self.db, def)
            )
        }
    }

    pub fn interface_ref_to_named_string(
        &self,
        names: &[String],
        interface_ref: InterfaceRef,
    ) -> String {
        let args = interface_ref.args(&self.db);
        let def_str = TypePrinter::new()
            .interface_id_to_string(&self.db, interface_ref.def(&self.db));
        if args.is_empty() {
            return def_str;
        }
        let args_str =
            args.iter().map(|ty| self.type_ref_to_named_string(names, *ty)).join(", ");
        format!("{def_str}<{args_str}>")
    }

    pub fn impl_id_to_named_string(&self, names: &[String], id: ImplId) -> String {
        let implemented = self.type_ref_to_named_string(names, id.implemented(&self.db));
        match id.interface(&self.db) {
            Some(inter) => {
                format!(
                    "<{implemented} as {}>",
                    self.interface_ref_to_named_string(names, inter)
                )
            }
            None => implemented,
        }
    }

    pub fn scope_owner_to_named_string(
        &self,
        names: &[String],
        scope_owner: ScopeOwnerId,
    ) -> String {
        match scope_owner {
            ScopeOwnerId::Module(module_id) => {
                TypePrinter::new().module_to_string(&self.db, module_id)
            }
            ScopeOwnerId::Impl(impl_id) => self.impl_id_to_named_string(names, impl_id),
            ScopeOwnerId::Interface(interface_ref) => {
                self.interface_ref_to_named_string(names, interface_ref)
            }
        }
    }

    pub fn function_sig_to_named_string(
        &self,
        names: &[String],
        sig: &FunctionSignature,
    ) -> String {
        let mut s = sig.name.display(&self.db).to_string();
        if !sig.added_templates.is_empty() || !sig.implicit_templates.is_empty() {
            let templates_str = sig
                .implicit_templates
                .iter()
                .chain(&sig.added_templates)
                .enumerate()
                .map(|(i, bounds)| {
                    let name = self.param_name(names, i);
                    if bounds.is_empty() {
                        name
                    } else {
                        format!(
                            "{name}: {}",
                            bounds
                                .iter()
                                .map(|c| self.interface_ref_to_named_string(names, *c))
                                .join(" + ")
                        )
                    }
                })
                .join(", ");
            s.push_str(&format!("<{templates_str}>"));
        }
        s.push('(');
        if let Some(arg) = sig.zelf {
            s.push_str(&arg.to_string());
            if !sig.args.is_empty() {
                s.push_str(", ");
            }
        }
        let args_str = sig
            .args
            .iter()
            .map(|(name, ty)| {
                format!(
                    "{}: {}",
                    name.display(&self.db),
                    self.type_ref_to_named_string(names, *ty)
                )
            })
            .join(", ");
        s.push_str(&args_str);
        s.push_str("): ");
        s.push_str(&self.type_ref_to_named_string(names, sig.ret));
        s
    }

    pub fn function_id_to_named_string(&self, func: FunctionId) -> String {
        let names = self.template_names(func.span(&self.db).start());
        let sig = get_sig_of_function(&self.db, func.interned());
        format!(
            "{}::{}",
            self.scope_owner_to_named_string(&names, func.parent(&self.db)),
            self.function_sig_to_named_string(&names, sig)
        )
    }
}
