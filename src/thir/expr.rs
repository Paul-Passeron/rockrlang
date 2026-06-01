use crate::{
    common::location::Span,
    thir::{ExprKind, PlaceId, ThirExpr, hir_to_thir::ThirBuilder},
};

impl ThirExpr {
    pub fn use_place(place: PlaceId, b: &ThirBuilder, span: Span) -> Self {
        Self {
            kind: ExprKind::Use(place),
            ty: b.get_place(place).ty,
            span,
        }
    }
}
