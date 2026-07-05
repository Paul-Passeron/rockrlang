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

use std::collections::HashMap;

use inkwell::context::Context;
use itertools::Itertools;

use crate::{
    codegen::{Codegen, LIRToLLVM, LTLLVMCtx, MIRToLIRBuild}, lir::{LIRFunctionId, build::FunctionBuilder}, thir_to_mir::FuncInst, unused,
};

pub struct MTLBCtx {
    pub mir_map: HashMap<FuncInst, LIRFunctionId>,
}

impl<'db> Codegen<'db, MIRToLIRBuild<'db>> {
    pub fn finalize(self) -> Codegen<'db, LIRToLLVM<'db>> {
        Codegen {
            db: self.db,
            lir: self.lir.finalize(),
            ctx: LTLLVMCtx { ctx: Context::create() },
        }
    }

    pub fn run(mut self) -> Codegen<'db, LIRToLLVM<'db>> {
        self.ctx
            .mir_map
            .values()
            .copied()
            .collect_vec()
            .into_iter()
            .for_each(|lir_id| self.lower(lir_id));
        self.finalize()
    }

    fn lower<'a>(&'a mut self, lir_id: LIRFunctionId) {
        let db = self.db;
        let Self { lir, ctx, .. } = self;
        lir.build_function(db, lir_id, |b| Self::_lower(b, ctx)).unwrap()
    }

    fn _lower<'ir>(builder: &mut FunctionBuilder<'ir, '_>, ctx: &mut MTLBCtx) {
        ctx.lower(builder);
    }
}

impl MTLBCtx {
    pub fn lower<'ir>(&mut self, b: &mut FunctionBuilder<'ir, '_>) {
        unused!(b);
        todo!()
    }
}
