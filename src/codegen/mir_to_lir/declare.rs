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

use crate::{
    Db,
    codegen::{Codegen, MIRToLIRBuild, MIRToLIRDeclare, MTLBCtx, mir_to_lir::build::MIRMap},
    layout::{LIRTy, layout_of},
    lir::{DefinedLinkage::Export, Module, Signature},
    mangle::fun_mangle,
    mir::{Mir, concrete_ty::ConcreteTy},
    thir_to_mir::FuncInst,
};

pub struct MTLDCtx<'a> {
    pub mir_map: MIRMap<'a>,
}

impl<'db, 'ctx> Codegen<'db, MIRToLIRDeclare<'db, 'ctx>> {
    pub fn new(db: &'db dyn Db) -> Self {
        Self { db, lir: Module::new(), ctx: MTLDCtx { mir_map: MIRMap::new() } }
    }

    pub fn finalize(self) -> Codegen<'db, MIRToLIRBuild<'db, 'ctx>> {
        Codegen {
            db: self.db,
            lir: self.lir.finish_declarations(),
            ctx: MTLBCtx { db: self.db, mir_map: self.ctx.mir_map },
        }
    }

    pub fn declare_import(
        &mut self,
        name: String,
        params: &[ConcreteTy],
        ret_ty: ConcreteTy,
        inst: FuncInst,
    ) {
        let params = params
            .iter()
            .map(|ty| LIRTy {
                layout: layout_of(self.db, ty.as_type_ref(self.db)),
                origin: Some(*ty),
            })
            .collect_vec();
        let ret_layout = layout_of(self.db, ret_ty.as_type_ref(self.db));
        let ret = LIRTy { layout: ret_layout, origin: Some(ret_ty) };
        let sig = Signature { params, ret };
        let variadic = inst.is_variadic(self.db);
        let id = self.lir().declare_import(name, sig, variadic);
        self.ctx.mir_map.add_import(inst, id);
    }

    pub fn declare_mir(&mut self, mir: &'db Mir) {
        let params = mir
            .func
            .params(self.db)
            .into_iter()
            .map(|(_, ty)| LIRTy {
                layout: layout_of(self.db, ty.as_type_ref(self.db)),
                origin: Some(ty),
            })
            .collect_vec();
        let ret_ty = mir.func.ret_ty(self.db);
        let ret_layout = layout_of(self.db, ret_ty.as_type_ref(self.db));
        let ret = LIRTy { layout: ret_layout, origin: Some(ret_ty) };
        let sig = Signature { params, ret };
        let inst = mir.func;
        let name = if inst.is_main(self.db) {
            "main".into()
        } else {
            fun_mangle(self.db, inst).mangle(self.db)
        };
        let id = self.lir().declare_defined(name, sig, Export);
        self.ctx.mir_map.add(mir, id);
    }
}
