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
    AddressSpace,
    attributes::{Attribute, AttributeLoc},
    basic_block::BasicBlock,
    builder::Builder,
    context::Context,
    module::Linkage,
    targets::TargetData,
    types::{AnyTypeEnum, BasicMetadataTypeEnum, BasicType, BasicTypeEnum, IntType},
    values::{AnyValue, BasicValue, BasicValueEnum, FunctionValue, PhiValue},
};
use itertools::Itertools;

use crate::{
    Db,
    codegen::{Codegen, IModule, LIRToLLVM},
    common::arena::Idx,
    layout::{Discriminant, IntWidth, LayoutData, LayoutID, Offset, ScalarKind},
    lir::{
        self, Complete, Finalized, FunctionSig, LIRDef, LIRFunctionId,
        finalized::{BlockData, FunctionBody, StackSlot},
        inst::{ConstValue, Instruction, Terminator, ValueInstKind, VoidInstKind},
    },
    ril::never_id,
};

impl<'db, 'ctx> Codegen<'db, LIRToLLVM<'db, 'ctx>> {
    pub fn finalize(self, ctx: &'ctx Context) -> IModule<'ctx> {
        let ctx = Ctx {
            db: self.db,
            lir: &self.lir,
            ctx,
            m: ctx.create_module("main"),
            b: ctx.create_builder(),
        };
        ctx.run()
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
    fn run(mut self) -> IModule<'ctx> {
        self.lir.functions().for_each(|func| self.declare_lir(func));
        self.lir.functions().for_each(|func| self.lower_lir(func));
        self.m
    }

    fn new_function_value(&mut self, sig: &FunctionSig) -> FunctionValue<'ctx> {
        let is_variadic = match sig.kind {
            lir::SigKind::Import { variadic } => variadic,
            lir::SigKind::Defined(_) => false,
        };
        let params = sig
            .signature
            .params
            .iter()
            .flat_map(|layout| {
                if layout.is_zst(self.db) {
                    None
                } else {
                    Some(
                        BasicMetadataTypeEnum::try_from(self.lower_layout(layout.layout))
                            .unwrap(),
                    )
                }
            })
            .collect_vec();
        let fn_ty = if !sig.signature.ret.is_zst(self.db) {
            self.basic(sig.signature.ret.layout).fn_type(&params, is_variadic)
        } else {
            self.ctx.void_type().fn_type(&params, is_variadic)
        };

        let f = self.m.add_function(
            &sig.name,
            fn_ty,
            match sig.kind {
                lir::SigKind::Import { .. } => None,
                lir::SigKind::Defined(defined_linkage) => Some(match defined_linkage {
                    lir::DefinedLinkage::Export => Linkage::External,
                    lir::DefinedLinkage::Local => Linkage::Private,
                    lir::DefinedLinkage::Weak => Linkage::ExternalWeak,
                }),
            },
        );

        if let Some(ty) = sig.signature.ret.origin
            && let Some(tid) = ty.as_type_id()
            && tid == never_id(self.db)
        {
            let kind_id = Attribute::get_named_enum_kind_id("noreturn");
            let noreturn = self.ctx.create_enum_attribute(kind_id, 0);
            f.add_attribute(AttributeLoc::Function, noreturn);
        }

        f
    }

    fn lower_layout(&mut self, layout: LayoutID) -> AnyTypeEnum<'ctx> {
        match layout.data(self.db) {
            LayoutData::ZeroSized => self.ctx.void_type().into(),
            LayoutData::Scalar(scalar_kind) => match scalar_kind {
                ScalarKind::Int(int_width) => self.get_int_ty(*int_width).into(),
                ScalarKind::Ptr => self.ctx.ptr_type(AddressSpace::default()).into(),
                ScalarKind::Float(_) => todo!(),
            },
            LayoutData::Aggregate(aggregate_layout) => {
                let field_types = &aggregate_layout
                    .fields
                    .iter()
                    .filter_map(|(_, ty)| {
                        BasicTypeEnum::try_from(self.lower_layout(*ty)).ok()
                    })
                    .collect_vec();
                self.ctx.struct_type(&field_types, false).into()
            }
            LayoutData::Union(_) => {
                self.ctx.i8_type().array_type(layout.size(self.db).bytes() as u32).into()
            }
        }
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
            lir::Body::Import { .. } => (), // We're done
            lir::Body::Defined(_, body) => {
                // Handle the entry differently
                for (blk, data) in &body.blocks {
                    if blk == body.entry {
                        let entry_bb = self.ctx.append_basic_block(fn_ctx.func, "entry");
                        fn_ctx.blocks.insert(blk, entry_bb);
                        self.b.position_at_end(entry_bb);

                        for slot in &body.stack_slots {
                            let StackSlot { value, pointee_ty } = slot;
                            let pointee = self.basic(pointee_ty.layout);
                            let ptr =
                                self.b.build_alloca(pointee, "alloca_slot").unwrap();
                            fn_ctx.values.insert(*value, ptr.into());
                        }

                        data.params
                            .iter()
                            .filter_map(|param| {
                                let ty = body.defs[*param].ty;
                                if ty.is_zst(self.db) { None } else { Some(*param) }
                            })
                            .enumerate()
                            .for_each(|(i, param)| {
                                fn_ctx.values.insert(
                                    param,
                                    fn_ctx.func.get_nth_param(i as u32).unwrap(),
                                );
                            });

                        for inst in &data.insts {
                            self.lower_inst(inst, &mut fn_ctx);
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
        ctx.blocks.insert(blk, bb);

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
            self.lower_inst(inst, ctx);
        }
    }

    fn lower_inst(&mut self, inst: &Instruction<Finalized>, ctx: &mut FnCtx<'ctx>) {
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
                let value: BasicValueEnum<'ctx> = match kind {
                    ValueInstKind::Const(const_value) => match const_value {
                        ConstValue::Int { ty, value } => {
                            let int_ty = self.basic(ty.layout).into_int_type();
                            int_ty
                                .const_int(
                                    *value as u64,
                                    true, /* FIXME: Do not hardcode signedness of the
                                           * int literals */
                                )
                                .into()
                        }
                        ConstValue::NullPtr { .. } => {
                            self.ctx.ptr_type(AddressSpace::default()).const_null().into()
                        }
                        ConstValue::Zeroed { ty } => self.basic(ty.layout).const_zero(),
                        ConstValue::Strlit { contents, null_terminated } => {
                            let array_value = self.ctx.const_string(
                                unescaper::unescape(contents).unwrap().as_bytes(),
                                *null_terminated,
                            );
                            let g = self.m.add_global(
                                array_value.get_type(),
                                None,
                                "global_str",
                            );
                            g.set_constant(true);
                            g.set_alignment(1);
                            g.set_linkage(Linkage::Private);
                            g.set_initializer(&array_value);
                            g.set_unnamed_addr(true);
                            g.as_pointer_value().as_basic_value_enum()
                        }
                        ConstValue::FunctionAddr(lirfunction_id) => {
                            let function = self.get_llvm_func(*lirfunction_id);
                            // TODO: is this right ?
                            function.as_any_value_enum().into_pointer_value().into()
                        }
                    },
                    ValueInstKind::Load { ptr, ty } => {
                        let ty = self.basic(ty.layout);
                        let ptr = ctx.values[ptr].into_pointer_value();
                        self.b.build_load(ty, ptr, "load_ptr").unwrap()
                    }
                    ValueInstKind::FieldPtr { ptr, ty, src_idx } => {
                        let ptr = ctx.values[ptr].into_pointer_value();
                        let (offset, _) = ty
                            .aggregate_layout(self.db)
                            .unwrap()
                            .source_field(*src_idx)
                            .unwrap();
                        if offset == Offset::ZERO {
                            ptr.as_basic_value_enum()
                        } else {
                            // expected to be array type of i8
                            let value = unsafe {
                                self.b
                                    .build_in_bounds_gep(
                                        self.ctx.i8_type(),
                                        ptr,
                                        &[self
                                            .ctx
                                            .i32_type()
                                            .const_int(offset.bytes(), false)],
                                        "offset_for_field_ptr",
                                    )
                                    .unwrap()
                            };
                            value.into()
                        }
                    }
                    ValueInstKind::UnionPayloadPtr { ptr, ty, .. } => {
                        let union_layout = ty.union_layout(self.db).unwrap();
                        let offset = union_layout.payload_offset;
                        let ptr = ctx.values[ptr].into_pointer_value();
                        unsafe {
                            self.b
                                .build_gep(
                                    self.ctx.i8_type(),
                                    ptr,
                                    &[self
                                        .ctx
                                        .i32_type()
                                        .const_int(offset.bytes(), false)],
                                    "payload_offset_ptr",
                                )
                                .unwrap()
                        }
                        .into()
                    }
                    ValueInstKind::GetDiscriminant { ptr, ty } => {
                        let layout = ty.union_layout(self.db).unwrap();
                        match layout.discriminant {
                            Discriminant::None => todo!(), /* Just return 0, */
                            // maybe ?
                            Discriminant::Niche { .. } => todo!(),
                            Discriminant::Tagged { offset, kind } => {
                                let tag_ptr =
                                    if offset == Offset::ZERO { ptr } else { todo!() };
                                let int_ty = self.get_int_ty(kind);
                                let tag_ptr = ctx.values[tag_ptr].into_pointer_value();
                                self.b.build_load(int_ty, tag_ptr, "the_discr").unwrap()
                            }
                        }
                    }
                    ValueInstKind::MakeAggregate { ty, fields_in_src_order } => {
                        let llvm_ty = self.basic(ty.layout);
                        let aggr_layout = ty.aggregate_layout(self.db).unwrap();
                        let mut res = llvm_ty.const_zero().into_struct_value();
                        for (i, to_layout) in
                            aggr_layout.source_to_layout.iter().enumerate()
                        {
                            if aggr_layout
                                .source_field(i as u32)
                                .unwrap()
                                .1
                                .is_zst(self.db)
                            {
                                continue;
                            }
                            let value = ctx.values[&fields_in_src_order[i as usize]];
                            res = self
                                .b
                                .build_insert_value(res, value, *to_layout, "")
                                .unwrap()
                                .into_struct_value();
                        }
                        res.into()
                    }
                    ValueInstKind::ExtractField { .. } => {
                        todo!()
                    }
                    ValueInstKind::InsertField { .. } => todo!(),
                    ValueInstKind::Arith { op, lhs, rhs } => {
                        let lhs = ctx.values[lhs].into_int_value();
                        let rhs = ctx.values[rhs].into_int_value();
                        match op {
                            lir::ArithBinop::SAdd | lir::ArithBinop::UAdd => {
                                self.b.build_int_add(lhs, rhs, "int-add").unwrap().into()
                            }
                            lir::ArithBinop::SMul | lir::ArithBinop::UMul => {
                                self.b.build_int_mul(lhs, rhs, "int-mul").unwrap().into()
                            }
                            lir::ArithBinop::UDiv => self
                                .b
                                .build_int_unsigned_div(lhs, rhs, "int-udiv")
                                .unwrap()
                                .into(),
                            lir::ArithBinop::SDiv => self
                                .b
                                .build_int_signed_div(lhs, rhs, "int-sdiv")
                                .unwrap()
                                .into(),
                            lir::ArithBinop::UMod => self
                                .b
                                .build_int_unsigned_rem(lhs, rhs, "int-umod")
                                .unwrap()
                                .into(),
                            lir::ArithBinop::SMod => self
                                .b
                                .build_int_signed_rem(lhs, rhs, "int-smod")
                                .unwrap()
                                .into(),
                            lir::ArithBinop::SSub | lir::ArithBinop::USub => {
                                self.b.build_int_sub(lhs, rhs, "int-sub").unwrap().into()
                            }
                        }
                    }
                    ValueInstKind::Cmp { op, lhs, rhs } => {
                        let lhs = ctx.values[lhs];
                        let rhs = ctx.values[rhs];
                        let (lhs, rhs) = if lhs.is_int_value() && rhs.is_int_value() {
                            let lhs = lhs.into_int_value();
                            let rhs = rhs.into_int_value();
                            (lhs, rhs)
                        } else if lhs.is_int_value() && rhs.is_pointer_value() {
                            let lhs = lhs.into_int_value();
                            let rhs = rhs.into_pointer_value();
                            let rhs =
                                self.b.build_ptr_to_int(rhs, lhs.get_type(), "").unwrap();
                            (lhs, rhs)
                        } else if lhs.is_pointer_value() && rhs.is_int_value() {
                            let rhs = rhs.into_int_value();
                            let lhs = lhs.into_pointer_value();
                            let lhs =
                                self.b.build_ptr_to_int(lhs, rhs.get_type(), "").unwrap();
                            (lhs, rhs)
                        } else if lhs.is_pointer_value() && rhs.is_pointer_value() {
                            let lhs = lhs.into_pointer_value();
                            let rhs = rhs.into_pointer_value();
                            let ptrint = self
                                .ctx
                                .ptr_sized_int_type(&TargetData::create(""), None);
                            let lhs = self.b.build_ptr_to_int(lhs, ptrint, "").unwrap();
                            let rhs = self.b.build_ptr_to_int(rhs, ptrint, "").unwrap();
                            (lhs, rhs)
                        } else {
                            todo!()
                        };

                        let op = match op {
                            lir::CmpBinop::SLessThan => inkwell::IntPredicate::SLT,
                            lir::CmpBinop::ULessThan => inkwell::IntPredicate::ULT,
                            lir::CmpBinop::Eq => inkwell::IntPredicate::EQ,
                        };
                        self.b.build_int_compare(op, lhs, rhs, "cmp").unwrap().into()
                    }
                    ValueInstKind::Logic { .. } => todo!(),
                    ValueInstKind::Not { value } => {
                        let value = ctx.values[value];
                        self.b
                            .build_not(value.into_int_value(), "logic_not")
                            .unwrap()
                            .into()
                    }
                    ValueInstKind::Cast { .. } => todo!(),
                    ValueInstKind::IndexPtr { ptr, elem_ty, index } => {
                        let ptr = ctx.values[ptr].into_pointer_value();
                        let idx = ctx.values[index].into_int_value();
                        let llvm_elem = self.basic(elem_ty.layout);
                        unsafe {
                            self.b
                                .build_gep(llvm_elem, ptr, &[idx], "index-ptr")
                                .unwrap()
                                .into()
                        }
                    }
                };
                ctx.values.insert(*def, value);
            }
            Instruction::Call { dest, id, args } => {
                let f = self.get_llvm_func(*id);
                let args = args.iter().map(|arg| ctx.values[arg].into()).collect_vec();
                let callsite = self.b.build_call(f, &args, "callsite").unwrap();
                if let Some(v) = dest {
                    let value =
                        BasicValueEnum::try_from(callsite.as_any_value_enum()).unwrap();
                    ctx.values.insert(*v, value);
                }
            }
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
        let llvm_block = ctx.blocks[&blk];
        self.b.position_at_end(llvm_block);
        match t {
            Terminator::Goto(block_target) => {
                block_target.params.iter().for_each(|arg| {
                    let value = ctx.values[arg];
                    let phi = ctx.block_params[arg];
                    phi.add_incoming(&[(&value, llvm_block)]);
                });
                let dst = ctx.blocks[&block_target.block];
                self.b.build_unconditional_branch(dst).unwrap();
            }
            Terminator::Br { cond, if_true, if_false } => {
                let cond = ctx.values[cond].into_int_value();
                let cond = self
                    .b
                    .build_int_compare(
                        inkwell::IntPredicate::NE,
                        cond,
                        cond.get_type().const_zero(),
                        "cond",
                    )
                    .unwrap();
                if_true.params.iter().for_each(|arg| {
                    let value = ctx.values[arg];
                    let phi = ctx.block_params[arg];
                    phi.add_incoming(&[(&value, llvm_block)]);
                });
                if_false.params.iter().for_each(|arg| {
                    let value = ctx.values[arg];
                    let phi = ctx.block_params[arg];
                    phi.add_incoming(&[(&value, llvm_block)]);
                });
                let then_block = ctx.blocks[&if_true.block];
                let else_block = ctx.blocks[&if_false.block];
                self.b.build_conditional_branch(cond, then_block, else_block).unwrap();
            }
            Terminator::Switch { on, branches, default } => {
                let value = ctx.values[on].into_int_value();
                let value_ty = value.get_type();
                default.params.iter().for_each(|arg| {
                    let value = ctx.values[arg];
                    let phi = ctx.block_params[arg];
                    phi.add_incoming(&[(&value, llvm_block)]);
                });
                let else_block = ctx.blocks[&default.block];
                let mut cases = vec![];
                for (value, block_target) in branches {
                    block_target.params.iter().for_each(|arg| {
                        let value = ctx.values[arg];
                        let phi = ctx.block_params[arg];
                        phi.add_incoming(&[(&value, llvm_block)]);
                    });
                    let int_value = value_ty.const_int(*value as u64, false);
                    cases.push((int_value, ctx.blocks[&block_target.block]));
                }
                self.b.build_switch(value, else_block, &cases).unwrap();
            }
            Terminator::Return(value) => match value {
                Some(v) => {
                    let value = ctx.values[v];
                    self.b.build_return(Some(&value)).unwrap();
                }
                None => {
                    self.b.build_return(None).unwrap();
                }
            },
            Terminator::Diverge => {
                self.b.build_unreachable().unwrap();
            }
        }
    }
}
