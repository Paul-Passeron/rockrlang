use crate::{
    Db,
    check::{Diag, Diagnostics},
    ril::ImplSource,
};

pub fn check_implem<'db>(db: &'db dyn Db, implem: ImplSource<'db>) -> Diagnostics<'db> {
    let span = implem.span(db);
    Diagnostics::new(
        db,
        vec![Diag::todo(db, "implement check_implem".into(), span)],
    )
}
