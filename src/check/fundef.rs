use crate::{
    Db,
    check::{Diag, Diagnostics},
    ril::FunctionId,
};

pub fn check_fundef<'db>(db: &'db dyn Db, fdef: FunctionId) -> Diagnostics<'db> {
    let span = fdef.name_span(db);
    Diagnostics::new(
        db,
        vec![Diag::todo(db, "implement check_fundef".into(), span)],
    )
}
