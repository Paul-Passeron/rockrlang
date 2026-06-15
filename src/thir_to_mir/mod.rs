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

use std::{collections::HashMap, sync::Arc};

use crate::{
    Db,
    mir::{MIR, MIRLocal, MIRLocalID, SyntacticSource, builder::MIRBuilder},
    ril::{FunctionId, TypeId, TypeRef},
    thir::{self, Thir, thir_body},
};

pub struct ThirToMIR<'a> {
    db: &'a dyn Db,
    builder: MIRBuilder<'a>,
    thir: &'a Thir,
    subs: &'a [TypeRef],

    local_map: HashMap<thir::LocalId, MIRLocalID>,
    params: Vec<MIRLocalID>,
}

#[salsa::interned]
pub struct MIRKey {
    fdef: FunctionId,

    #[returns(ref)]
    subs: Vec<TypeRef>,
}

/// Wrapper to send MIR safely between threads as it is supposed to be read-only
#[derive(Clone, PartialEq, Eq)]
struct _MIRWrapper(Arc<MIR>);
unsafe impl Sync for _MIRWrapper {}
unsafe impl Send for _MIRWrapper {}

#[salsa::tracked]
fn _mir<'db>(db: &'db dyn Db, key: MIRKey<'db>) -> _MIRWrapper {
    let Some(thir) = thir_body(db, key.fdef(db)) else {
        panic!("attempted to lower extern function to MIR")
    };
    _MIRWrapper(Arc::new(
        ThirToMIR::new(db, thir.as_ref(), key.subs(db)).lower(),
    ))
}

pub fn mir(db: &dyn Db, fdef: FunctionId, subs: Vec<TypeRef>) -> Arc<MIR> {
    _mir(db, MIRKey::new(db, fdef, subs)).0
}

impl<'a> ThirToMIR<'a> {
    pub fn new(db: &'a dyn Db, thir: &'a Thir, subs: &'a [TypeRef]) -> Self {
        Self {
            db,
            builder: MIRBuilder::new(db, Some("entry".into())),
            thir,
            subs,
            local_map: HashMap::new(),
            params: Vec::new(),
        }
    }

    pub fn is_valid_ty(&self, ty: TypeRef) -> bool {
        match ty {
            TypeRef::Concrete(type_id) => type_id
                .args(self.db)
                .into_iter()
                .all(|ty| self.is_valid_ty(ty)),
            TypeRef::Param(_) => false,
            TypeRef::Associated(_) => todo!(),
            _ => false,
        }
    }

    /// Returns the substituted form of the type
    pub fn ty(&self, ty: TypeRef) -> TypeRef {
        let res = match ty {
            TypeRef::Concrete(type_id) => TypeRef::Concrete(TypeId::new(
                self.db,
                type_id.def(self.db),
                type_id
                    .args(self.db)
                    .into_iter()
                    .map(|ty| self.ty(ty))
                    .collect(),
            )),
            TypeRef::Param(id) => self.subs[id.0],
            TypeRef::Zelf => {
                let unsub = self
                    .thir
                    .id
                    .parent(self.db)
                    .get_canonical_zelf(self.db)
                    .unwrap_or(TypeRef::Error);
                if matches!(unsub, TypeRef::Zelf) {
                    // Do not recurse infinitely, bail early
                    TypeRef::Error
                } else {
                    self.ty(unsub)
                }
            }
            TypeRef::Associated(_) => todo!(),
            _ => ty,
        };
        if !self.is_valid_ty(res) {
            panic!("Not a valid ty")
        }
        res
    }

    fn build_thir_locals(&mut self) {
        for (thir_id, local) in self.thir.locals.iter() {
            let ty = self.ty(local.ty);
            let mut mir = MIRLocal::new(ty, local.mutability, local.span);
            if let Some(src) = &local.source {
                mir = mir.with_name(src.1).with_thir_src(thir_id).with_syn_src(
                    SyntacticSource {
                        span: local.span,
                        id: src.0,
                    },
                )
            }
            let mir_id = self.builder.new_local(mir);

            self.local_map.insert(thir_id, mir_id);
        }
    }

    fn build_arguments(&mut self) {
        for param in &self.thir.params {
            let mir_id = *self.local_map.get(param).expect("Expected all local thir ids to be found in self.local_map. Maybe try calling `build_thir_locals`.");
            self.params.push(mir_id);
        }
    }

    fn check_substitution(&self) {
        for ty in self.subs {
            if !self.is_valid_ty(*ty) {
                panic!(
                    "Expecting a valid type in substitution but got {}",
                    ty.to_string(self.db)
                )
            }
        }
    }

    pub fn lower(mut self) -> MIR {
        self.check_substitution();
        self.build_thir_locals();
        self.build_arguments();
        todo!()
    }
}
