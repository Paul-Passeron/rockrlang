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

use std::collections::HashSet;

use itertools::Itertools;

use crate::{
    Db,
    name_resolve::builtin_module,
    ril::{BuiltinTypeId, ModuleId, PtrKind, TypeDefId},
    thir::inference::{InferTy, InferenceCtx},
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

    pub fn with_options(opts: impl IntoIterator<Item = TypePrinterOption>) -> Self {
        Self {
            options: TypePrinterOptionSet::with_options(opts),
        }
    }

    fn is_def_builtin(db: &dyn Db, def: TypeDefId) -> Option<BuiltinTypeId> {
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

    pub fn type_def_id_to_string(&self, db: &dyn Db, id: TypeDefId) -> String {
        if self.options.has(TypePrinterOption::PrintPath) {
            let mut res = String::new();
            fn _aux(db: &dyn Db, opts: &TypePrinterOptionSet, s: &mut String, module: ModuleId) {
                if module == builtin_module(db) && !opts.has(TypePrinterOption::PrintBuiltin) {
                    return;
                }
                if let Some(parent) = module.parent(db) {
                    _aux(db, opts, s, parent);
                }
                s.push_str(&module.name(db).to_string(db));
            }

            _aux(db, &self.options, &mut res, id.parent(db));
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
                    && let Some(id) = Self::is_def_builtin(db, def)
                    && let Some((prefix, suffix)) = Self::is_builtin_pretty_print(db, id)
                {
                    return format!("{}{}{}", prefix, fields_str, suffix);
                }
                if no_fields {
                    format!("{}", self.type_def_id_to_string(db, def))
                } else {
                    format!("{}<{fields_str}>", self.type_def_id_to_string(db, def),)
                }
            }
            InferTy::Param(type_param_id) => format!("T{}", type_param_id.0),
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
        Self::with_options([TypePrinterOption::PrettyPrintBuiltinADTs])
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
