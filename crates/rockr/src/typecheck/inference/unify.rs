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

use super::{InferTy, InferVar, TypeInferenceError, UnifyValue};
use crate::typecheck::inference::InferenceCtx;
use ena::unify::{InPlace, UnificationTable};

impl InferTy {
    fn occurs(&self, var: InferVar) -> bool {
        match self {
            Self::Var(this_var) => *this_var == var,
            Self::Adt { args: fields, .. } => {
                fields.iter().any(|field| field.occurs(var))
            }
            Self::Param(_) | Self::Error => false,
        }
    }

    fn unify(&self, other: &Self) -> Result<Self, TypeInferenceError> {
        fn unify_aux(
            a: &InferTy,
            b: &InferTy,
            flag: bool,
        ) -> Result<InferTy, TypeInferenceError> {
            match (a, b) {
                (InferTy::Var(var_a), InferTy::Var(_)) => Ok(InferTy::Var(*var_a)),
                (
                    InferTy::Adt { def: def_a, args: fields_a },
                    InferTy::Adt { def: def_b, args: fields_b },
                ) => {
                    if def_a != def_b {
                        Err(TypeInferenceError::TypeDefIdMismatch(*def_a, *def_b))
                    } else if let (len_a, len_b) = (fields_a.len(), fields_b.len())
                        && len_a != len_b
                    {
                        Err(TypeInferenceError::FieldCountMismatch(len_a, len_b))
                    } else {
                        let fields = fields_a
                            .iter()
                            .zip(fields_b)
                            .map(|(field_a, field_b)| field_a.unify(field_b))
                            .collect::<Result<_, _>>()?;
                        Ok(InferTy::Adt { def: *def_a, args: fields })
                    }
                }
                (InferTy::Adt { def, args: fields }, InferTy::Var(infer_var)) => {
                    if fields.iter().any(|field| field.occurs(*infer_var)) {
                        Err(TypeInferenceError::RecursiveDefinition(*infer_var))
                    } else {
                        Ok(InferTy::Adt { def: *def, args: fields.clone() })
                    }
                }
                (InferTy::Param(p), InferTy::Var(_))
                | (InferTy::Var(_), InferTy::Param(p)) => Ok(InferTy::Param(*p)),
                (InferTy::Param(pa), InferTy::Param(pb)) => {
                    if pa == pb {
                        Ok(InferTy::Param(*pa))
                    } else {
                        Err(TypeInferenceError::TemplateConstraining(*pa))
                    }
                }
                (InferTy::Param(p), _) | (_, InferTy::Param(p)) => {
                    Err(TypeInferenceError::TemplateConstraining(*p))
                }
                _ if !flag => unify_aux(b, a, true),
                _ => panic!(
                    "Infinite recursion detected, You might need to handle more cases explicitely"
                ),
            }
        }
        unify_aux(self, other, false)
    }
}

impl UnifyValue for InferTy {
    type Error = TypeInferenceError;

    fn unify_values(a: &Self, b: &Self) -> Result<Self, <Self as UnifyValue>::Error> {
        a.unify(b)
    }
}

impl InferenceCtx<'_> {
    fn merge_listeners(&mut self, a: InferVar, b: InferVar) {
        let root = self.table.find(a);
        let other = if root == a { b } else { a };
        self.solver.merge_listeners(other, root);
    }
    fn try_unify(&mut self, a: &InferTy, b: &InferTy) -> Result<(), TypeInferenceError> {
        let a = &self.find(a);
        let b = &self.find(b);
        match (a, b) {
            (InferTy::Var(a), InferTy::Error) | (InferTy::Error, InferTy::Var(a)) => {
                self.table.unify_var_value(*a, Some(InferTy::Error))
            }
            (_, InferTy::Error) | (InferTy::Error, _) => Ok(()),
            (InferTy::Var(a), InferTy::Var(b)) => self
                .table
                .unify_var_var(*a, *b)
                .inspect(|()| self.merge_listeners(*a, *b)),
            (InferTy::Var(infer_var), value) | (value, InferTy::Var(infer_var)) => {
                let resolved = self.find(value);
                if resolved.occurs(*infer_var) {
                    return Err(TypeInferenceError::RecursiveDefinition(*infer_var));
                }
                self.table.unify_var_value(*infer_var, Some(resolved))?;
                let root = self.table.find(*infer_var);
                self.solver.wake(root);
                Ok(())
            }
            (
                InferTy::Adt { def: def_a, args: fields_a },
                InferTy::Adt { def: def_b, args: fields_b },
            ) => {
                if def_a != def_b {
                    Err(TypeInferenceError::TypeDefIdMismatch(*def_a, *def_b))
                } else if let (len_a, len_b) = (fields_a.len(), fields_b.len())
                    && len_a != len_b
                {
                    Err(TypeInferenceError::FieldCountMismatch(len_a, len_b))
                } else {
                    fields_a
                        .iter()
                        .zip(fields_b)
                        .try_for_each(|(field_a, field_b)| self.unify(field_a, field_b))
                }
            }
            (InferTy::Param(pa), InferTy::Param(pb)) => {
                (pa == pb).ok_or(TypeInferenceError::TemplateConstraining(*pa))
            }
            (InferTy::Param(p), _) | (_, InferTy::Param(p)) => {
                Err(TypeInferenceError::TemplateConstraining(*p))
            }
        }
    }

    pub fn unify(&mut self, a: &InferTy, b: &InferTy) -> Result<(), TypeInferenceError> {
        self.snapshot(|this| this.try_unify(a, b))
    }

    pub fn show(&mut self, ty: &InferTy) -> String {
        self.find(ty).to_string(self.db)
    }

    pub fn find(&mut self, ty: &InferTy) -> InferTy {
        match ty {
            InferTy::Var(infer_var) => {
                let infer_var = self.table.find(*infer_var);
                match self.table.probe_value(infer_var) {
                    Some(v) => self.find(&v),
                    None => InferTy::Var(infer_var),
                }
            }
            InferTy::Adt { def, args: fields } => InferTy::Adt {
                def: *def,
                args: fields.iter().map(|ty| self.find(ty)).collect(),
            },
            InferTy::Param(type_param_id) => InferTy::Param(*type_param_id),
            InferTy::Error => InferTy::Error,
        }
    }

    pub fn find_const(&self, ty: &InferTy) -> InferTy {
        fn find(
            ty: &InferTy,
            table: &mut UnificationTable<InPlace<InferVar>>,
        ) -> InferTy {
            match ty {
                InferTy::Var(infer_var) => {
                    let infer_var = table.find(*infer_var);
                    match table.probe_value(infer_var) {
                        Some(v) => find(&v, table),
                        None => infer_var.into(),
                    }
                }
                InferTy::Adt { def, args: fields } => InferTy::Adt {
                    def: *def,
                    args: fields.iter().map(|ty| find(ty, table)).collect(),
                },
                InferTy::Param(type_param_id) => InferTy::Param(*type_param_id),
                InferTy::Error => InferTy::Error,
            }
        }
        // For the moment, ena does not let us find the value without the table
        // being mutable for path-compression reason I believe. We might
        // want to find an alternative.
        let mut tbl = self.table.clone();
        find(ty, &mut tbl)
    }
}
