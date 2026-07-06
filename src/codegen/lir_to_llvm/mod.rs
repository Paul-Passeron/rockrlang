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

use inkwell::{
    builder::Builder,
    context::Context,
    module::Linkage,
    types::{AnyTypeEnum, BasicMetadataTypeEnum, BasicType, BasicTypeEnum},
    values::FunctionValue,
};
use itertools::Itertools;

use crate::{
    Db,
    codegen::{Codegen, IModule, LIRToLLVM},
    layout::LayoutID,
    lir::{self, Complete, FunctionSig, LIRFunctionId},
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
    fn run(mut self) {
        self.lir.functions().for_each(|func| self.declare_lir(func));
        self.lir.functions().for_each(|func| self.lower_lir(func));
    }

    fn new_function_value(&mut self, sig: &FunctionSig) -> FunctionValue<'ctx> {
        let params = sig
            .signature
            .params
            .iter()
            .flat_map(|layout| {
                if layout.is_zst(self.db) {
                    None
                } else {
                    Some(
                        BasicMetadataTypeEnum::try_from(
                            self.lower_layout(layout.layout),
                        )
                        .unwrap(),
                    )
                }
            })
            .collect_vec();
        let fn_ty = if sig.signature.ret.is_zst(self.db) {
            BasicTypeEnum::try_from(self.lower_layout(sig.signature.ret.layout))
                .unwrap()
                .fn_type(&params, false)
        } else {
            self.ctx.void_type().fn_type(&params, false)
        };
        self.m.add_function(
            &sig.name,
            fn_ty,
            match sig.kind {
                lir::SigKind::Import => None,
                lir::SigKind::Defined(defined_linkage) => {
                    Some(match defined_linkage {
                        lir::DefinedLinkage::Export => Linkage::External,
                        lir::DefinedLinkage::Local => Linkage::Private,
                        lir::DefinedLinkage::Weak => Linkage::ExternalWeak,
                    })
                }
            },
        )
    }

    fn lower_layout(&mut self, layout: LayoutID) -> AnyTypeEnum<'ctx> {
        todo!()
    }

    fn get_llvm_func(&self, l: LIRFunctionId) -> FunctionValue<'ctx> {
        self.m.get_function(&self.lir.get_fn(l).0.name).unwrap()
    }

    fn declare_lir(&mut self, l: LIRFunctionId) {
        let (sig, _) = self.lir.get_fn(l);
        let new_fn_value = self.new_function_value(sig);
    }

    fn lower_lir(&mut self, l: LIRFunctionId) {
        let (sig, body) = self.lir.get_fn(l);
    }
}
