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

use inkwell::{builder::Builder, context::Context};

use crate::{
    Db,
    codegen::{Codegen, IModule, LIRToLLVM},
    lir::{self, Complete, LIRFunctionId},
    unused,
};

impl<'db> Codegen<'db, LIRToLLVM<'db>> {
    pub(super) fn finalize(self) -> Context {
        self.ctx.ctx
    }

    pub fn run(self) -> Context {
        let ctx = Ctx {
            db: self.db,
            lir: &self.lir,
            ctx: &self.ctx.ctx,
            m: self.ctx.ctx.create_module("main"),
            b: self.ctx.ctx.create_builder(),
        };
        ctx.run();
        self.finalize()
    }
}

#[allow(unused)]
struct Ctx<'db, 'lir, 'ctx> {
    db: &'db dyn Db,
    lir: &'lir lir::Module<Complete>,
    ctx: &'ctx Context,
    m: IModule<'ctx>,
    b: Builder<'ctx>,
}

impl<'db, 'lir, 'ctx> Ctx<'db, 'lir, 'ctx> {
    fn run(self) {
        self.lir.functions().for_each(|func| self.lower_lir(func));
    }

    fn lower_lir(&self, l: LIRFunctionId) {
        unused!(l);
    }
}
