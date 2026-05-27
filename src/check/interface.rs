use crate::{
    Db,
    check::{Diag, Diagnostics},
    ril::InterfaceId,
};

pub fn check_interface<'db>(db: &'db dyn Db, interface: InterfaceId) -> Diagnostics<'db> {
    let span = interface.name_span(db);
    Diagnostics::new(
        db,
        vec![Diag::todo(db, "implement check_interface".into(), span)],
    )
}
