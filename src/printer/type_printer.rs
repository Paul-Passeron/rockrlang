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
use std::collections::HashSet;
use std::fmt::Write;

use crate::{
    Db,
    compiler::{FunctionSignature, get_sig_of_function},
    name_resolve::{builtin_module, definition::Definition},
    ril::{
        BuiltinTypeId, FunctionId, ImplId, InterfaceId, InterfaceRef, ModuleId, PtrKind,
        ScopeOwnerId, TypeDefId, TypeId, TypeParamId, TypeRef,
    },
    typecheck::inference::{InferTy, InferenceCtx},
};

pub struct TypePrinter {
    pub options: TypePrinterOptionSet,
}

impl TypePrinter {
    pub fn new() -> Self {
        Self {
            options: Default::default(),
        }
    }

    #[allow(dead_code)]
    pub fn with_options(opts: impl IntoIterator<Item = TypePrinterOption>) -> Self {
        Self {
            options: TypePrinterOptionSet::with_options(opts),
        }
    }

    fn is_def_builtin(def: TypeDefId) -> Option<BuiltinTypeId> {
        match def {
            TypeDefId::Builtin(id) => Some(id),
            _ => None,
        }
    }

    fn is_builtin_pretty_print(db: &dyn Db, def: BuiltinTypeId) -> Option<(String, String)> {
        if let Some(ptrkid) = def.is_ptr_like(db) {
            let muta = match ptrkid {
                PtrKind::Ref(mutability) | PtrKind::RawPtr(mutability) => mutability,
            };
            let muta_suffix = match muta {
                crate::hir::Mutability::Const => "",
                crate::hir::Mutability::Mutable => "mut ",
            };
            let prefix = match ptrkid {
                PtrKind::Ref(_) => "&",
                PtrKind::RawPtr(_) => "*",
            };
            Some((format!("{prefix}{muta_suffix}"), String::new()))
        } else if def == BuiltinTypeId::slice(db) {
            Some((String::from("["), String::from("]")))
        } else if def == BuiltinTypeId::tuple(db) {
            Some((String::from("("), String::from(")")))
        } else {
            None
        }
    }

    pub fn module_to_string(&self, db: &dyn Db, module: ModuleId) -> String {
        let mut res = String::new();
        fn _aux(db: &dyn Db, opts: &TypePrinterOptionSet, s: &mut String, module: ModuleId) {
            if module == builtin_module(db) && !opts.has(TypePrinterOption::PrintBuiltin) {
                return;
            }
            if let Some(parent) = module.parent(db) {
                _aux(db, opts, s, parent);
                if !s.is_empty() {
                    s.push_str("::");
                }
            }
            s.push_str(&module.name(db).to_string(db));
        }
        _aux(db, &self.options, &mut res, module);
        res
    }

    pub fn type_def_id_to_string(&self, db: &dyn Db, id: TypeDefId) -> String {
        if self.options.has(TypePrinterOption::PrintPath) {
            let mut res = self.module_to_string(db, id.parent(db));
            if !res.is_empty() {
                res.push_str("::");
            }
            res.push_str(&id.name(db).to_string(db));
            res
        } else {
            id.name(db).to_string(db)
        }
    }

    pub fn infer_ty_to_string(
        &self,
        db: &dyn Db,
        infer_ty: InferTy,
        ctx: Option<&InferenceCtx>,
    ) -> String {
        let infer_ty = if let Some(ctx) = ctx {
            ctx.find_const(&infer_ty)
        } else {
            infer_ty
        };
        match infer_ty {
            InferTy::Var(infer_var) => {
                if self.options.has(TypePrinterOption::DebugInferenceVars) {
                    infer_var.to_string()
                } else {
                    String::from("_")
                }
            }
            InferTy::Adt { def, fields } => {
                let no_fields = fields.is_empty();
                let fields_str = fields
                    .into_iter()
                    .map(|ty| self.infer_ty_to_string(db, ty, ctx))
                    .collect_vec()
                    .join(", ");
                if self.options.has(TypePrinterOption::PrettyPrintBuiltinADTs)
                    && let Some(id) = Self::is_def_builtin(def)
                    && let Some((prefix, suffix)) = Self::is_builtin_pretty_print(db, id)
                {
                    format!("{}{}{}", prefix, fields_str, suffix)
                } else if no_fields {
                    format!("{}", self.type_def_id_to_string(db, def))
                } else {
                    format!("{}<{fields_str}>", self.type_def_id_to_string(db, def),)
                }
            }
            InferTy::Param(type_param_id) => format!("T{}", type_param_id.0),
        }
    }

    pub fn interface_id_to_string(&self, db: &dyn Db, id: InterfaceId) -> String {
        let mut res = self.module_to_string(db, id.parent(db));
        if !res.is_empty() {
            res.push_str("::");
        }
        res.push_str(&id.name(db).to_string(db));
        res
    }

    pub fn impl_id_to_string(&self, db: &dyn Db, id: ImplId) -> String {
        let mut res = self.module_to_string(db, id.parent(db));
        if !res.is_empty() {
            res.push_str("::");
        }
        res.push_str("`impl ");

        let templates = id.templates(db);
        let templates = templates
            .iter()
            .enumerate()
            .map(|(i, s)| {
                format!(
                    "T{i}{}",
                    if s.is_empty() {
                        String::new()
                    } else {
                        format!(
                            ": {}",
                            s.iter()
                                .map(|interface| self.interface_ref_to_string(db, *interface))
                                .collect_vec()
                                .join(" + ")
                        )
                    }
                )
            })
            .collect_vec()
            .join(", ");

        if !templates.is_empty() {
            res.push('<');
            res.push_str(&templates);
            res.push('>');
        }

        if let Some(inter) = id.interface(db) {
            res.push_str(&self.interface_ref_to_string(db, inter));
            res.push_str(" ");
        }
        res.push('`');
        res
    }

    pub fn interface_ref_to_string(&self, db: &dyn Db, interface_ref: InterfaceRef) -> String {
        let args = interface_ref.args(db);
        let args_str = args
            .iter()
            .map(|arg| self.type_ref_to_string(db, *arg))
            .collect_vec()
            .join(", ");
        format!(
            "{}{}",
            self.interface_id_to_string(db, interface_ref.def(db)),
            if args.is_empty() {
                args_str
            } else {
                format!("<{args_str}>")
            }
        )
    }

    pub fn type_id_to_string(&self, db: &dyn Db, type_id: TypeId) -> String {
        let fields = type_id.args(db);
        let def = type_id.def(db);
        let no_fields = fields.is_empty();
        let fields_str = fields
            .into_iter()
            .map(|ty| self.type_ref_to_string(db, ty))
            .collect_vec()
            .join(", ");
        if self.options.has(TypePrinterOption::PrettyPrintBuiltinADTs)
            && let Some(id) = Self::is_def_builtin(def)
            && let Some((prefix, suffix)) = Self::is_builtin_pretty_print(db, id)
        {
            format!("{}{}{}", prefix, fields_str, suffix)
        } else if no_fields {
            format!("{}", self.type_def_id_to_string(db, def))
        } else {
            format!("{}<{fields_str}>", self.type_def_id_to_string(db, def),)
        }
    }

    pub fn type_param_id_to_string(&self, _db: &dyn Db, type_ref: TypeParamId) -> String {
        format!("T{}", type_ref.0)
    }

    pub fn type_ref_to_string(&self, db: &dyn Db, type_ref: TypeRef) -> String {
        match type_ref {
            TypeRef::Concrete(type_id) => self.type_id_to_string(db, type_id),
            TypeRef::Associated(symbol) => format!("Self::{}", symbol.to_string(db)),
            TypeRef::Param(type_param_id) => self.type_param_id_to_string(db, type_param_id),
            TypeRef::Zelf => format!("Self"),
            TypeRef::Error => format!("<ERROR>"),
            TypeRef::Unknown => format!("<???>"),
        }
    }

    pub fn scope_owner_to_string(&self, db: &dyn Db, scope_owner: ScopeOwnerId) -> String {
        match scope_owner {
            ScopeOwnerId::Module(module_id) => self.module_to_string(db, module_id),
            ScopeOwnerId::Impl(impl_id) => self.impl_id_to_string(db, impl_id),
            ScopeOwnerId::Interface(interface_ref) => {
                self.interface_ref_to_string(db, interface_ref)
            }
        }
    }

    pub fn function_sig_to_string(&self, db: &dyn Db, sig: &FunctionSignature) -> String {
        let mut s = String::new();
        let f = &mut s;
        let mut aux = || -> std::fmt::Result {
            write!(f, "{}", sig.name.display(db))?;
            if !sig.added_templates.is_empty() || !sig.implicit_templates.is_empty() {
                write!(f, "<")?;
                for (i, t) in sig
                    .implicit_templates
                    .iter()
                    .chain(&sig.added_templates)
                    .enumerate()
                {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "T{i}")?;
                    if !t.is_empty() {
                        write!(
                            f,
                            ": {}",
                            t.iter()
                                .map(|c| self.interface_ref_to_string(db, *c))
                                .collect_vec()
                                .join(" + ")
                        )?;
                    }
                }
                write!(f, ">")?;
            }
            write!(f, "(")?;
            if let Some(arg) = sig.zelf {
                write!(f, "{arg}")?;
                if !sig.args.is_empty() {
                    write!(f, ", ")?;
                }
            }
            write!(
                f,
                "{}): {}",
                sig.args
                    .iter()
                    .map(|arg| format!(
                        "{}: {}",
                        arg.0.display(db),
                        self.type_ref_to_string(db, arg.1)
                    ))
                    .collect_vec()
                    .join(", "),
                self.type_ref_to_string(db, sig.ret)
            )?;
            Ok(())
        };
        aux().unwrap();
        s
    }

    pub fn function_id_to_string(&self, db: &dyn Db, function_id: FunctionId) -> String {
        let sig = get_sig_of_function(db, function_id.interned());
        format!(
            "{}::{}",
            self.scope_owner_to_string(db, function_id.parent(db)),
            self.function_sig_to_string(db, sig.as_ref())
        )
    }

    pub fn called_function_to_string(&self, db: &dyn Db, function_id: FunctionId) -> String {
        format!(
            "{}::{}",
            self.scope_owner_to_string(db, function_id.parent(db)),
            function_id.name(db).display(db)
        )
    }

    pub fn definition_to_string(&self, db: &dyn Db, def: Definition) -> String {
        match def {
            Definition::Function(function_id) => self.function_id_to_string(db, function_id),
            Definition::Interface(interface_id) => self.interface_id_to_string(db, interface_id),
            Definition::Module(module_id) => self.module_to_string(db, module_id),
            Definition::Type(type_def_id) => self.type_def_id_to_string(db, type_def_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypePrinterOptionSet {
    options: HashSet<TypePrinterOption>,
}

impl TypePrinterOptionSet {
    pub fn has(&self, opt: TypePrinterOption) -> bool {
        self.options.contains(&opt)
    }

    pub fn with_options(opts: impl IntoIterator<Item = TypePrinterOption>) -> Self {
        Self {
            options: HashSet::from_iter(opts),
        }
    }

    pub fn with(self, opt: TypePrinterOption) -> Self {
        let mut this = self;
        this.options.insert(opt);
        this
    }
}

impl Default for TypePrinterOptionSet {
    fn default() -> Self {
        Self::with_options([
            TypePrinterOption::PrettyPrintBuiltinADTs,
            TypePrinterOption::PrintPath,
        ])
    }
}

// TODO: use bit flags instead
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypePrinterOption {
    /// Print as a fully-qualified path from the outermost scope to the innermost ex: `core::io::str`
    PrintPath,

    /// Print the `@builtin` outer-scope when printing path
    PrintBuiltin,

    /// Wether or not to debug print inference variables. Ex: `Vec<_>` can become `Vec<'19>`
    DebugInferenceVars,

    /// Pretty-prints ADT types like tuple and references.
    /// Ex: `(int, str, str)` might be printed as `()<int, int, str>` if this flag not enabled.
    PrettyPrintBuiltinADTs,
}
