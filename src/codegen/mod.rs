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
}

pub struct MIRToLIRDeclare<'db, 'ctx>(
    PhantomData<&'db ()>,
    PhantomData<&'ctx ()>,
);
pub struct MIRToLIRBuild<'db, 'ctx>(
    PhantomData<&'db ()>,
    PhantomData<&'ctx ()>,
);
pub struct LIRToLLVM<'db, 'ctx>(PhantomData<&'db ()>, PhantomData<&'ctx ()>);

impl<'db, 'ctx> CGState<'db> for MIRToLIRDeclare<'db, 'ctx> {
    type Ctx = MTLDCtx<'db>;

    type LIRState = Declaring;

    type Out = Codegen<'db, MIRToLIRBuild<'db, 'ctx>>;
}

impl<'db, 'ctx> CGState<'db> for MIRToLIRBuild<'db, 'ctx> {
    type Ctx = MTLBCtx<'db>;

    type LIRState = Building;
    type Out = Codegen<'db, LIRToLLVM<'db, 'ctx>>;
}

impl<'db, 'ctx> CGState<'db> for LIRToLLVM<'db, 'ctx> {
    type Ctx = ();

    type LIRState = Complete;

    type Out = IModule<'ctx>;
}

impl<'db, 'ctx> Codegen<'db, MIRToLIRDeclare<'db, 'ctx>> {
    pub fn lir(&mut self) -> &mut Module<Declaring> {
        &mut self.lir
    }
}

impl<'db, 'ctx> Codegen<'db, MIRToLIRBuild<'db, 'ctx>> {
    pub fn lir(&mut self) -> &mut Module<Building> {
        &mut self.lir
    }
}

impl<'db, 'ctx> Codegen<'db, LIRToLLVM<'db, 'ctx>> {
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
