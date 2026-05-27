use crate::{
    Db,
    check::{Diag, Diagnostics},
    name_resolve::definition::Definition,
    ril::TypeDefId,
};

pub fn check_typedef<'db>(db: &'db dyn Db, typedef: TypeDefId) -> Diagnostics<'db> {
    if let Some(span) = typedef.name_span(db) {
        return Diagnostics::new(
            db,
            vec![Diag::todo(db, "implement check_typedef".into(), span)],
        );
    }
    Diagnostics::new(db, vec![])
}
