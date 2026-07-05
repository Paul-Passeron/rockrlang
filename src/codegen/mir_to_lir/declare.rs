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

use crate::{
    codegen::{Codegen, MIRToLIRBuild, MIRToLIRDeclare, MTLBCtx},
    lir::LIRFunctionId,
    thir_to_mir::FuncInst,
};

pub struct MTLDCtx {
    pub mir_map: HashMap<FuncInst, LIRFunctionId>,
}

impl<'db> Codegen<'db, MIRToLIRDeclare<'db>> {
    pub fn finalize(self) -> Codegen<'db, MIRToLIRBuild<'db>> {
        Codegen {
            db: self.db,
            lir: self.lir.finish_declarations(),
            ctx: MTLBCtx { mir_map: self.ctx.mir_map },
        }
    }
}
