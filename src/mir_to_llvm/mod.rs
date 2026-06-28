use std::{collections::HashMap, path::Path};

use inkwell::{
    OptimizationLevel,
    builder::Builder,
    context::Context,
    module::{Linkage, Module},
    targets::{
        CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
    },
    types::{AnyTypeEnum, BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType},
    values::FunctionValue,
};
use itertools::Itertools;

use crate::{
    Db, mir::MIR, mir_to_llvm::mir::MIRGen, ril::TypeRef, thir_to_mir::FuncInst,
};

pub mod mir;
pub mod types;

pub struct LLVMCtx<'a, 'db> {
    pub db: &'db dyn Db,
    pub c: &'a Context,
    pub m: Module<'a>,
    pub b: Builder<'a>,
    pub fun_map: HashMap<FuncInst, FunctionValue<'a>>,
}

impl<'a, 'db> LLVMCtx<'a, 'db> {
    pub fn ty(&self, ty: TypeRef) -> AnyTypeEnum<'a> {
        ty.as_type_id().unwrap().interned().as_llvm(self.db, self.c)
    }
}

trait MyFnType<'a> {
    fn fn_type(
        &self,
        param_types: &[BasicMetadataTypeEnum<'a>],
        is_var_args: bool,
    ) -> FunctionType<'a>;
}

impl<'a> MyFnType<'a> for AnyTypeEnum<'a> {
    fn fn_type(
        &self,
        param_types: &[BasicMetadataTypeEnum<'a>],
        is_var_args: bool,
    ) -> FunctionType<'a> {
        if let Ok(ty) = BasicTypeEnum::try_from(*self) {
            return ty.fn_type(param_types, is_var_args);
        }
        if self.is_void_type() {
            return self.into_void_type().fn_type(param_types, is_var_args);
        }
        todo!()
    }
}

impl<'a, 'db> LLVMCtx<'a, 'db> {
    pub fn new(db: &'db dyn Db, c: &'a Context, frefs: &[FuncInst]) -> Self {
        let m = c.create_module("main");
        let b = c.create_builder();

        let mut this = Self {
            db,
            c,
            m,
            b,
            fun_map: HashMap::new(),
        };

        let fun_map = frefs
            .iter()
            .map(|fref| {
                let ty = this.ty_of_fref(*fref);
                let name = fref.get_mangled_name(this.db);
                let f = this.m.get_function(&name).unwrap_or_else(|| {
                    this.m.add_function(
                        name.as_str(),
                        ty,
                        if fref.fdef(db).has_body(db) {
                            Some(Linkage::External)
                        } else {
                            None
                        },
                    )
                });
                (*fref, f)
            })
            .collect();

        this.fun_map = fun_map;
        this
    }

    fn ty_of_fref(&self, func: FuncInst) -> FunctionType<'a> {
        let ret_ty = self.ty(func.ret_ty(self.db));
        let arg_tys = func
            .params(self.db)
            .iter()
            .map(|ty| self.ty(ty.1))
            .map(|ty| BasicTypeEnum::try_from(ty).unwrap())
            .map(BasicMetadataTypeEnum::from)
            .collect_vec();

        ret_ty.fn_type(&arg_tys, func.fdef(self.db).is_var_args(self.db))
    }

    pub fn lower_mir(&self, mir: &MIR) {
        MIRGen::new(self, mir).lower();
    }

    pub fn write_object_file(&self, path: &Path) -> Result<(), String> {
        self.m.verify().map_err(|e| e.to_string())?;

        Target::initialize_native(&InitializationConfig::default())
            .map_err(|e| e.to_string())?;

        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;

        let target_machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                OptimizationLevel::Default,
                RelocMode::Default,
                CodeModel::Default,
            )
            .ok_or_else(|| "failed to create target machine".to_string())?;

        self.m.set_triple(&triple);
        self.m
            .set_data_layout(&target_machine.get_target_data().get_data_layout());

        target_machine
            .write_to_file(&self.m, FileType::Object, path)
            .map_err(|e| e.to_string())
    }
}
