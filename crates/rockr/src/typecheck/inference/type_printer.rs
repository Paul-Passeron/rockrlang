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
    Db,
    resolved::type_printer::{TypePrinter, TypePrinterOption},
    typecheck::inference::{InferTy, InferenceCtx},
};
use itertools::Itertools;

impl TypePrinter {
    pub fn infer_ty_to_string(
        &self,
        db: &dyn Db,
        infer_ty: InferTy,
        ctx: Option<&InferenceCtx>,
    ) -> String {
        let infer_ty =
            if let Some(ctx) = ctx { ctx.find_const(&infer_ty) } else { infer_ty };
        match infer_ty {
            InferTy::Var(infer_var) => {
                if self.options.has(TypePrinterOption::DebugInferenceVars) {
                    infer_var.to_string()
                } else {
                    String::from("_")
                }
            }
            InferTy::Adt { def, args: fields } => {
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
                    format!("{prefix}{fields_str}{suffix}")
                } else if no_fields {
                    self.type_def_id_to_string(db, def)
                } else {
                    format!("{}<{fields_str}>", self.type_def_id_to_string(db, def))
                }
            }
            InferTy::Param(type_param_id) => format!("T{}", type_param_id.0),
            InferTy::Error => "<ERROR>".to_owned(),
        }
    }
}
