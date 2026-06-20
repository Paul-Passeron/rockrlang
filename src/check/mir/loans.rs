use std::collections::HashSet;

use crate::{
    Db,
    mir::{
        MIR, MIRBlockID,
        analysis::loans::{LoanID, MIRLoanOut},
        basic_block::{MIRTerminator, Stmt},
    },
};

pub(super) fn check_loans(db: &dyn Db, mir: &MIR) {
    let loans = mir.loans(db);
    for idx in mir.blocks.keys() {
        check_block(db, mir, idx, loans);
    }
}

fn check_block(db: &dyn Db, mir: &MIR, blk: MIRBlockID, loans: &MIRLoanOut) {
    let mut state = loans.loans_live_in[&blk].clone();
    let block = &mir.blocks[blk];
    block
        .stmts
        .iter()
        .for_each(|stmt| check_stmt(db, mir, stmt, loans, &mut state));
    check_terminator(db, mir, &block.terminator, loans, &mut state);
}

fn check_stmt(
    db: &dyn Db,
    mir: &MIR,
    stmt: &Stmt,
    loans: &MIRLoanOut,
    state: &mut HashSet<LoanID>,
) {
    let Stmt::Assign { dest, rvalue } = stmt;
}

fn check_terminator(
    db: &dyn Db,
    mir: &MIR,
    terminator: &MIRTerminator,
    loans: &MIRLoanOut,
    state: &mut HashSet<LoanID>,
) {
    todo!()
}
