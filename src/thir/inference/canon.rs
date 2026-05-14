use std::fmt::{self};

use crate::{
    Db,
    ril::{
        TypeDefId,
        display::{Display, RilDisplay},
    },
    thir::inference::{InferTy, InferenceCtx},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CanonTy {
    Hole,
    Adt { id: TypeDefId, args: Box<[CanonTy]> },
    Param(usize),
}

impl<'db> InferenceCtx<'db> {
    pub fn canonize(&mut self, ty: &InferTy) -> CanonTy {
        match self.find(ty) {
            InferTy::Var(_) => CanonTy::Hole,
            InferTy::Adt { def, fields } => CanonTy::Adt {
                id: def,
                args: fields.iter().map(|f| self.canonize(f)).collect(),
            },
            InferTy::Param(type_param_id) => CanonTy::Param(type_param_id.0),
        }
    }
}

impl CanonTy {
    pub fn display(&self, db: &dyn Db) -> impl fmt::Display {
        Display { value: self, db }
    }
}

impl fmt::Display for Display<'_, &CanonTy> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            CanonTy::Hole => write!(f, "_"),
            CanonTy::Adt { id, args } => {
                write!(f, "{}", id.display(self.db))?;
                if !args.is_empty() {
                    write!(
                        f,
                        "({})",
                        args.iter()
                            .map(|a| a.display(self.db).to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )?;
                }
                Ok(())
            }
            CanonTy::Param(n) => write!(f, "T{}", n),
        }
    }
}
