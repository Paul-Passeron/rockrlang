use salsa::Accumulator;

use crate::{Db, compiler::diagnostic::Diag, thir::Thir};

pub fn sanity_check(db: &dyn Db, thir: &Thir) {
    Diag::todo(
        "Implement the sanity check on thir.".into(),
        thir.body_span(db),
    )
    .accumulate(db);
}
