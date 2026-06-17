use crate::{
    Db,
    mir::{MIRBlockID, operand::MIRPlace},
    ril::{TypeDefId, TypeId, TypeRef},
    thir::{EnumRef, ThirMatchBranch},
    thir_to_mir::ThirToMIR,
    unused,
};

pub(super) struct MatchLowerer<'a, 'b> {
    db: &'b dyn Db,
    ctx: &'a mut ThirToMIR<'b>,
    scrut: MIRPlace,
    merge_bb: MIRBlockID,
}

impl<'a, 'b> MatchLowerer<'a, 'b> {
    pub fn new(
        ctx: &'a mut ThirToMIR<'b>,
        scrut: MIRPlace,
        merge_bb: MIRBlockID,
    ) -> Self {
        Self {
            db: ctx.db,
            ctx,
            scrut,
            merge_bb,
        }
    }

    fn lower_enum(
        &mut self,
        enum_ref: EnumRef,
        depth: usize,
        branches: &[ThirMatchBranch],
    ) {
        unused!(branches);
        todo!(
            "Deref enum_ref {} with depth={depth}",
            enum_ref.as_type_ref(self.db).to_string(self.db)
        )
    }

    fn lower_any(&mut self, branches: &[ThirMatchBranch]) {
        unused!(branches);
        unused!(self.ctx);
        unused!(self.merge_bb);
        todo!()
    }

    pub fn lower(&mut self, branches: &[ThirMatchBranch]) {
        let (peeled, depth) = self.scrut.ty.peel_aux(self.db);
        if let Some(enum_ref) = peeled.as_enum_ref(self.db) {
            self.lower_enum(enum_ref, depth, branches);
        } else {
            self.lower_any(branches);
        }
    }
}

impl EnumRef {
    pub fn as_type_ref(self, db: &dyn Db) -> TypeRef {
        TypeId::new(db, TypeDefId::Enum(self.def), self.args).into()
    }
}
