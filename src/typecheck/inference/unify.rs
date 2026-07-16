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

use ena::unify::{InPlace, UnificationTable};
use itertools::Itertools;

use crate::typecheck::inference::InferenceCtx;

use super::{InferTy, InferVar, UnificationError, UnifyValue};

impl InferTy {
    fn occurs(&self, var: InferVar) -> bool {
        match self {
            InferTy::Var(this_var) => *this_var == var,
            InferTy::Adt { fields, .. } => fields.iter().any(|field| field.occurs(var)),
            InferTy::Param(_) => false,
        }
    }

    fn unify(&self, other: &Self) -> Result<Self, UnificationError> {
        fn _unify(
            a: &InferTy,
            b: &InferTy,
            flag: bool,
        ) -> Result<InferTy, UnificationError> {
            match (a, b) {
                (InferTy::Var(var_a), InferTy::Var(_)) => Ok(InferTy::Var(*var_a)),
                (
                    InferTy::Adt { def: def_a, fields: fields_a },
                    InferTy::Adt { def: def_b, fields: fields_b },
                ) => {
                    if def_a != def_b {
                        Err(UnificationError::TypeDefIdMismatch(*def_a, *def_b))
                    } else if let (len_a, len_b) = (fields_a.len(), fields_b.len())
                        && len_a != len_b
                    {
                        Err(UnificationError::FieldCountMismatch(len_a, len_b))
                    } else {
                        let fields = fields_a
                            .iter()
                            .zip(fields_b)
                            .map(|(field_a, field_b)| field_a.unify(field_b))
                            .collect::<Result<_, _>>()?;
                        Ok(InferTy::Adt { def: *def_a, fields })
                    }
                }
                (InferTy::Adt { def, fields }, InferTy::Var(infer_var)) => {
                    if fields.iter().any(|field| field.occurs(*infer_var)) {
                        Err(UnificationError::RecursiveDefinition(*infer_var))
                    } else {
                        Ok(InferTy::Adt { def: *def, fields: fields.clone() })
                    }
                }
                (InferTy::Param(p), InferTy::Var(_))
                | (InferTy::Var(_), InferTy::Param(p)) => Ok(InferTy::Param(*p)),
                (InferTy::Param(pa), InferTy::Param(pb)) => {
                    if pa == pb {
                        Ok(InferTy::Param(*pa))
                    } else {
                        Err(UnificationError::TemplateConstraining(*pa))
                    }
                }
                (InferTy::Param(p), _) | (_, InferTy::Param(p)) => {
                    Err(UnificationError::TemplateConstraining(*p))
                }
                _ if !flag => _unify(b, a, true),
                _ => panic!(
                    "Infinite recursion detected, You might need to handle more cases explicitely"
                ),
            }
        }
        _unify(self, other, false)
    }
}

impl UnifyValue for InferTy {
    type Error = UnificationError;

    fn unify_values(a: &Self, b: &Self) -> Result<Self, Self::Error> {
        a.unify(b)
    }
}

impl<'db> InferenceCtx<'db> {
    fn merge_listeners(&mut self, a: InferVar, b: InferVar) {
        let root = self.table.find(a);
        let other = if root == a { b } else { a };
        let flattened = self.listeners.remove(&other).into_iter().flatten().collect_vec();
        self.listeners.entry(root).or_default().extend(flattened);
    }
    fn try_unify(&mut self, a: &InferTy, b: &InferTy) -> Result<(), UnificationError> {
        let a = &self.find(a);
        let b = &self.find(b);
        match (a, b) {
            (InferTy::Var(a), InferTy::Var(b)) => {
                self.table.unify_var_var(*a, *b).inspect(|_| self.merge_listeners(*a, *b))
            }
            (InferTy::Var(infer_var), value) | (value, InferTy::Var(infer_var)) => {
                let resolved = self.find(value);
                if resolved.occurs(*infer_var) {
                    return Err(UnificationError::RecursiveDefinition(*infer_var));
                }
                self.table.unify_var_value(*infer_var, Some(resolved))?;
                self.listeners
                    .remove(&self.table.find(*infer_var))
                    .into_iter()
                    .flatten()
                    .for_each(|l| {
                        if self.ready_set.insert(l) {
                            self.ready.push_back(l);
                        }
                    });
                Ok(())
            }
            (
                InferTy::Adt { def: def_a, fields: fields_a },
                InferTy::Adt { def: def_b, fields: fields_b },
            ) => {
                if def_a != def_b {
                    Err(UnificationError::TypeDefIdMismatch(*def_a, *def_b))
                } else if let (len_a, len_b) = (fields_a.len(), fields_b.len())
                    && len_a != len_b
                {
                    Err(UnificationError::FieldCountMismatch(len_a, len_b))
                } else {
                    fields_a.iter().zip(fields_b).try_for_each(|(field_a, field_b)| {
                        self.unify(field_a.clone(), field_b.clone())
                    })
                }
            }
            (InferTy::Param(pa), InferTy::Param(pb)) => {
                (pa == pb).ok_or(UnificationError::TemplateConstraining(*pa))
            }
            (InferTy::Param(p), _) | (_, InferTy::Param(p)) => {
                Err(UnificationError::TemplateConstraining(*p))
            }
        }
    }

    pub fn unify(&mut self, a: InferTy, b: InferTy) -> Result<(), UnificationError> {
        self.snapshot(|this| this.try_unify(&a, &b))
    }

    pub fn find(&mut self, ty: &InferTy) -> InferTy {
        match ty {
            InferTy::Var(infer_var) => {
                let infer_var = self.table.find(*infer_var);
                self.table.probe_value(infer_var).unwrap_or(ty.clone())
            }
            InferTy::Adt { def, fields } => InferTy::Adt {
                def: *def,
                fields: fields.iter().map(|ty| self.find(ty)).collect(),
            },
            InferTy::Param(type_param_id) => InferTy::Param(*type_param_id),
        }
    }

    pub fn find_const(&self, ty: &InferTy) -> InferTy {
        fn _find(
            ty: &InferTy,
            table: &mut UnificationTable<InPlace<InferVar>>,
        ) -> InferTy {
            match ty {
                InferTy::Var(infer_var) => {
                    let infer_var = table.find(*infer_var);
                    table.probe_value(infer_var).unwrap_or(ty.clone())
                }
                InferTy::Adt { def, fields } => InferTy::Adt {
                    def: *def,
                    fields: fields.iter().map(|ty| _find(ty, table)).collect(),
                },
                InferTy::Param(type_param_id) => InferTy::Param(*type_param_id),
            }
        }
        // For the moment, ena does not let us find the value without the table
        // being mutable for path-compression reason I believe. We might
        // want to find an alternative.
        let mut tbl = self.table.clone();
        _find(ty, &mut tbl)
    }
}
