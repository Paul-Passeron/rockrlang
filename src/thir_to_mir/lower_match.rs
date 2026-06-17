use crate::{
    Db,
    mir::{MIRBlockID, operand::MIRPlace},
    ril::{TypeDefId, TypeId, TypeRef},
    thir::{EnumRef, ThirMatchBranch},
    thir_to_mir::{
        ThirToMIR,
        decision_tree::{DecisionTree, Matrix, Row},
    },
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

    fn build_match_matrix(&mut self, branches: &'a [ThirMatchBranch]) -> Matrix<'a> {
        let rows = branches
            .iter()
            .enumerate()
            .map(|(idx, branch)| -> Row<'a> {
                assert!(branch.guard.is_none()); // TODO
                Row {
                    pats: vec![Some(&branch.pattern)],
                    branch_idx: idx,
                    bindings: vec![],
                }
            })
            .collect();

        Matrix {
            cols: vec![self.scrut.clone()],
            rows,
        }
    }

    fn lower_decision_tree(&mut self, dt: &DecisionTree, branches: &[ThirMatchBranch]) {
        todo!()
    }

    pub fn lower(&mut self, branches: &'a [ThirMatchBranch]) {
        let matrix = self.build_match_matrix(branches);
        let decision_tree = matrix.compile(self.ctx);
        println!("Tree obtained: {:#?}", decision_tree);
        self.lower_decision_tree(&decision_tree, branches);
    }
}

impl EnumRef {
    pub fn as_type_ref(self, db: &dyn Db) -> TypeRef {
        TypeId::new(db, TypeDefId::Enum(self.def), self.args).into()
    }
}
