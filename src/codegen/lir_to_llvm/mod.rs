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

use inkwell::{
    basic_block::BasicBlock,
    builder::Builder,
    context::Context,
    module::Linkage,
    types::{
        AnyTypeEnum, BasicMetadataTypeEnum, BasicType, BasicTypeEnum, IntType,
    },
    values::{BasicValueEnum, FunctionValue, PhiValue},
};
use itertools::Itertools;

use crate::{
    Db,
    codegen::{Codegen, IModule, LIRToLLVM},
    common::arena::Idx,
    layout::{Discriminant, IntWidth, LayoutID, Offset},
    lir::{
        self, Complete, Finalized, FunctionSig, LIRDef, LIRFunctionId,
        finalized::{BlockData, FunctionBody},
        inst::{Instruction, Terminator, ValueInstKind, VoidInstKind},
    },
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

struct Ctx<'db, 'lir, 'ctx> {
    db: &'db dyn Db,
    lir: &'lir lir::Module<Complete>,
    ctx: &'ctx Context,
    m: IModule<'ctx>,
    b: Builder<'ctx>,
}

struct FnCtx<'ctx> {
    func: FunctionValue<'ctx>,

    block_params: HashMap<Idx<LIRDef>, PhiValue<'ctx>>,
    blocks: HashMap<Idx<BlockData>, BasicBlock<'ctx>>,
    values: HashMap<Idx<LIRDef>, BasicValueEnum<'ctx>>,
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
        let fn_ty = if !sig.signature.ret.is_zst(self.db) {
            self.basic(sig.signature.ret.layout).fn_type(&params, false)
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
        self.new_function_value(sig);
    }

    fn lower_lir(&mut self, l: LIRFunctionId) {
        let (_, body) = self.lir.get_fn(l);
        let mut fn_ctx = FnCtx {
            func: self.get_llvm_func(l),
            block_params: HashMap::new(),
            blocks: HashMap::new(),
            values: HashMap::new(),
        };
        match body {
            lir::Body::Import => (), // We're done
            lir::Body::Defined(_, body) => {
                // Handle the entry differently
                for (blk, data) in &body.blocks {
                    if blk == body.entry {
                        let entry_bb =
                            self.ctx.append_basic_block(fn_ctx.func, "entry");
                        self.b.position_at_end(entry_bb);
                        data.params
                            .iter()
                            .filter_map(|param| {
                                let ty = body.defs[*param].ty;
                                if ty.is_zst(self.db) {
                                    None
                                } else {
                                    Some(*param)
                                }
                            })
                            .enumerate()
                            .for_each(|(i, param)| {
                                fn_ctx.values.insert(
                                    param,
                                    fn_ctx
                                        .func
                                        .get_nth_param(i as u32)
                                        .unwrap(),
                                );
                            });

                        for inst in &data.insts {
                            self.lower_inst(inst, body, &mut fn_ctx);
                        }
                    } else {
                        self.build_block(blk, body, &mut fn_ctx);
                    }
                }
                for (blk, data) in &body.blocks {
                    self.terminate_block(blk, &data.terminator, &mut fn_ctx);
                }
            }
        }
    }

    fn basic(&mut self, layout: LayoutID) -> BasicTypeEnum<'ctx> {
        BasicTypeEnum::try_from(self.lower_layout(layout)).unwrap()
    }

    fn build_block(
        &mut self,
        blk: Idx<BlockData>,
        body: &FunctionBody,
        ctx: &mut FnCtx<'ctx>,
    ) {
        let bb = self.ctx.append_basic_block(ctx.func, "");
        self.b.position_at_end(bb);

        let blk_data = &body.blocks[blk];

        blk_data
            .params
            .iter()
            .filter_map(|param| {
                let ty = body.defs[*param].ty;
                if ty.is_zst(self.db) { None } else { Some((*param, ty)) }
            })
            .enumerate()
            .for_each(|(i, (param, ty))| {
                let llvm_ty = self.basic(ty.layout);
                if !ty.is_zst(self.db) {
                    let phi_node = self
                        .b
                        .build_phi(
                            llvm_ty,
                            format!("blk_{}_param_{i}", blk.raw()).as_str(),
                        )
                        .unwrap();
                    ctx.block_params.insert(param, phi_node);
                }
            });

        for inst in &blk_data.insts {
            self.lower_inst(inst, body, ctx);
        }
    }

    fn lower_inst(
        &mut self,
        inst: &Instruction<Finalized>,
        body: &FunctionBody,
        ctx: &mut FnCtx<'ctx>,
    ) {
        match inst {
            Instruction::Void(void_inst_kind) => match void_inst_kind {
                VoidInstKind::Store { ptr, value } => {
                    let ptr = ctx.values[ptr].into_pointer_value();
                    let value = ctx.values[value];
                    self.b.build_store(ptr, value).unwrap();
                }
                VoidInstKind::MemCopy { src, dst, ty } => {
                    let dest = ctx.values[dst].into_pointer_value();
                    let src = ctx.values[src].into_pointer_value();
                    let align = ty.layout.align(self.db).bytes() as u32;
                    let size = self
                        .ctx
                        .i64_type()
                        .const_int(ty.layout.size(self.db).bytes(), false);
                    self.b.build_memcpy(dest, align, src, align, size).unwrap();
                }
                VoidInstKind::SetDiscriminant { ptr, ty, idx } => {
                    let vlay = ty.union_layout(self.db).unwrap();
                    match vlay.discriminant {
                        Discriminant::None => (),
                        Discriminant::Tagged { offset, kind } => {
                            if offset == Offset::ZERO {
                                let ptr = ctx.values[ptr].into_pointer_value();
                                self.b
                                    .build_store(
                                        ptr,
                                        self.get_int_ty(kind)
                                            .const_int(*idx as u64, false),
                                    )
                                    .unwrap();
                            } else {
                                todo!()
                            }
                        }
                        Discriminant::Niche { .. } => todo!(),
                    }
                }
            },
            Instruction::Value { def, kind } => {
                let layout = body.defs[*def].ty.layout;
                let value: BasicValueEnum<'ctx> = match kind {
                    ValueInstKind::Const(const_value) => {
                        todo!()
                    }
                    ValueInstKind::Load { ptr, ty } => todo!(),
                    ValueInstKind::FieldPtr { ptr, ty, src_idx } => todo!(),
                    ValueInstKind::UnionPayloadPtr { ptr, ty, variant } => {
                        todo!()
                    }
                    ValueInstKind::GetDiscriminant { ptr, ty } => todo!(),
                    ValueInstKind::MakeAggregate {
                        ty,
                        fields_in_src_order,
                    } => todo!(),
                    ValueInstKind::ExtractField { value, ty, src_idx } => {
                        todo!()
                    }
                    ValueInstKind::InsertField {
                        value,
                        ty,
                        src_idx,
                        field,
                    } => todo!(),
                    ValueInstKind::Arith { op, lhs, rhs } => todo!(),
                    ValueInstKind::Cmp { op, lhs, rhs } => todo!(),
                    ValueInstKind::Logic { op, lhs, rhs } => todo!(),
                    ValueInstKind::Not { value } => todo!(),
                    ValueInstKind::Cast { kind, value, to } => todo!(),
                };
                ctx.values.insert(*def, value);
            }
            Instruction::Call { dest, id, args } => todo!(),
        }
    }

    fn get_int_ty(&self, w: IntWidth) -> IntType<'ctx> {
        match w {
            IntWidth::I8 => self.ctx.i8_type(),
            IntWidth::I16 => self.ctx.i16_type(),
            IntWidth::I32 => self.ctx.i32_type(),
            IntWidth::I64 => self.ctx.i64_type(),
            IntWidth::I128 => self.ctx.i128_type(),
        }
    }

    fn terminate_block(
        &mut self,
        blk: Idx<BlockData>,
        t: &Terminator<Finalized>,
        ctx: &mut FnCtx<'ctx>,
    ) {
        todo!()
    }
}

// First instructions of every block with nodes:
// phi nodes that we'll extend at each jump to the target with the block params
// passed We have to have a map from block to phi nodes in param order in the
// FnCtx This scales nicely with the args (No special cases for no args, etc...)
// This requires two passes though (Setting up all phi nodes) and then
// terminating each block, patching the phi nodes on the target blocks
// accordingly
