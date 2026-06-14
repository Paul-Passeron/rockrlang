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

use std::sync::Arc;

use crate::{
    Db,
    mir::{MIR, builder::MIRBuilder},
    ril::{FunctionId, TypeRef},
    thir::{Thir, thir_body},
};

pub struct ThirToMIR<'a> {
    db: &'a dyn Db,
    builder: MIRBuilder<'a>,
    thir: &'a Thir,
    subs: &'a [TypeRef],
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
        }
    }

    pub fn lower(self) -> MIR {
        todo!()
    }
}
