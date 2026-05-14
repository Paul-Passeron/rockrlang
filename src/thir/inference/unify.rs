use crate::{ril::display::RilDisplay, thir::inference::InferenceCtx};

use super::{InferTy, InferVar, UnificationError, UnifyValue};

impl InferTy {
    fn occurs(&self, var: InferVar) -> bool {
        match self {
            InferTy::Var(this_var) => *this_var == var,
            InferTy::Adt { fields, .. } => fields.iter().any(|field| field.occurs(var)),
            InferTy::Param(_) | InferTy::Zelf => false,
        }
    }

    fn unify(&self, other: &Self) -> Result<Self, UnificationError> {
        fn _unify(a: &InferTy, b: &InferTy, flag: bool) -> Result<InferTy, UnificationError> {
            match (a, b) {
                (InferTy::Var(var_a), InferTy::Var(_)) => Ok(InferTy::Var(*var_a)),
                (
                    InferTy::Adt {
                        def: def_a,
                        fields: fields_a,
                    },
                    InferTy::Adt {
                        def: def_b,
                        fields: fields_b,
                    },
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
                        Ok(InferTy::Adt {
                            def: *def_a,
                            fields,
                        })
                    }
                }
                (InferTy::Adt { def, fields }, InferTy::Var(infer_var)) => {
                    if fields.iter().any(|field| field.occurs(*infer_var)) {
                        Err(UnificationError::RecursiveDefinition(*infer_var))
                    } else {
                        Ok(InferTy::Adt {
                            def: *def,
                            fields: fields.clone(),
                        })
                    }
                }
                (InferTy::Param(p), InferTy::Var(_)) | (InferTy::Var(_), InferTy::Param(p)) => {
                    Ok(InferTy::Param(*p))
                }
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
    fn try_unify(&mut self, a: &InferTy, b: &InferTy) -> Result<(), UnificationError> {
        let a = &self.find(a);
        let b = &self.find(b);
        match (a, b) {
            (InferTy::Zelf, InferTy::Zelf) => Ok(()),
            (InferTy::Var(a), InferTy::Var(b)) => self.table.unify_var_var(*a, *b),
            (InferTy::Var(infer_var), value) | (value, InferTy::Var(infer_var)) => {
                self.table.unify_var_value(*infer_var, Some(value.clone()))
            }
            (
                InferTy::Adt {
                    def: def_a,
                    fields: fields_a,
                },
                InferTy::Adt {
                    def: def_b,
                    fields: fields_b,
                },
            ) => {
                if def_a != def_b {
                    println!("{} != {}", def_a.display(self.db), def_b.display(self.db));
                    Err(UnificationError::TypeDefIdMismatch(*def_a, *def_b))
                } else if let (len_a, len_b) = (fields_a.len(), fields_b.len())
                    && len_a != len_b
                {
                    Err(UnificationError::FieldCountMismatch(len_a, len_b))
                } else {
                    fields_a
                        .iter()
                        .zip(fields_b)
                        .try_for_each(|(field_a, field_b)| {
                            self.unify(field_a.clone(), field_b.clone())
                        })
                }
            }
            (InferTy::Param(pa), InferTy::Param(pb)) => {
                if pa == pb {
                    Ok(())
                } else {
                    Err(UnificationError::TemplateConstraining(*pa))
                }
            }
            (InferTy::Param(p), _) | (_, InferTy::Param(p)) => {
                Err(UnificationError::TemplateConstraining(*p))
            }
            (InferTy::Zelf, _) | (_, InferTy::Zelf) => Err(UnificationError::ZelfConstraining),
        }
    }

    pub fn unify(&mut self, a: InferTy, b: InferTy) -> Result<(), UnificationError> {
        self.snapshot(|this| {
            this.try_unify(&a, &b)?;
            this.solve_constraints()
                .map_err(|(inference_constraint, unification_error)| {
                    UnificationError::UnmetConstraint(
                        inference_constraint,
                        Box::new(unification_error),
                    )
                })
        })
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
            InferTy::Zelf => InferTy::Zelf,
        }
    }
}
