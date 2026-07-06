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

use std::marker::PhantomData;

use inkwell::context::Context;

use crate::{
    Db,
    codegen::mir_to_lir::{build::MTLBCtx, declare::MTLDCtx},
    lir::{Building, Complete, Declaring, Module, ModulePhase},
};

mod lir_to_llvm;
mod mir_to_lir;

pub type IModule<'a> = inkwell::module::Module<'a>;

pub struct Codegen<'db, State: CGState<'db>> {
    db: &'db dyn Db,
    lir: Module<State::LIRState>,
    ctx: State::Ctx,
}

pub trait CGState<'db>: Sized {
    type Ctx;
    type LIRState: ModulePhase;
    type Out;

    fn finalize(cg: Codegen<'db, Self>) -> Self::Out;
}

pub struct MIRToLIRDeclare<'db>(PhantomData<&'db ()>);
pub struct MIRToLIRBuild<'db>(PhantomData<&'db ()>);
pub struct LIRToLLVM<'db>(PhantomData<&'db ()>);

pub struct LTLLVMCtx {
    pub ctx: Context,
}

impl<'db> CGState<'db> for MIRToLIRDeclare<'db> {
    type Ctx = MTLDCtx<'db>;

    type LIRState = Declaring;

    type Out = Codegen<'db, MIRToLIRBuild<'db>>;

    fn finalize(cg: Codegen<'db, Self>) -> Self::Out {
        cg.finalize()
    }
}

impl<'db> CGState<'db> for MIRToLIRBuild<'db> {
    type Ctx = MTLBCtx<'db>;

    type LIRState = Building;
    type Out = Codegen<'db, LIRToLLVM<'db>>;

    fn finalize(cg: Codegen<'db, Self>) -> Self::Out {
        cg.finalize()
    }
}

impl<'db> CGState<'db> for LIRToLLVM<'db> {
    type Ctx = LTLLVMCtx;

    type LIRState = Complete;

    type Out = Context;

    fn finalize(cg: Codegen<'db, Self>) -> Self::Out {
        cg.finalize()
    }
}

impl<'db> Codegen<'db, MIRToLIRDeclare<'db>> {
    pub fn lir(&mut self) -> &mut Module<Declaring> {
        &mut self.lir
    }
}

impl<'db> Codegen<'db, MIRToLIRBuild<'db>> {
    pub fn lir(&mut self) -> &mut Module<Building> {
        &mut self.lir
    }
}

impl<'db> Codegen<'db, LIRToLLVM<'db>> {
    pub fn lir(&self) -> &Module<Complete> {
        &self.lir
    }
}

impl<'db, S: CGState<'db>> Codegen<'db, S> {
    pub fn ctx(&mut self) -> &mut S::Ctx {
        &mut self.ctx
    }

    pub fn db(&self) -> &'db dyn Db {
        self.db
    }
}
