/* Rockr programming language
Copyright (C) 2026  NoRezap

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

use std::collections::HashSet;

use itertools::Itertools;

use crate::{
    Db,
    check::thir::sanity_check::ConstructorType,
    common::symbols::Symbol,
    mir::{
        MIRLocalID,
        operand::{MIRPlace, MIRProjection},
    },
    name_resolve::type_expr::enum_item,
    parse_tree::top_level::AstEnumVariantKind,
    ril::{TypeRef, bool_id},
    thir::{ThirConstructorArgs, ThirPattern, ThirPatternKind},
    thir_to_mir::ThirToMIR,
};

// Compiling pattern matching to decision tree, inspired by
// http://moscova.inria.fr/~maranget/papers/ml05e-maranget.pdf

#[derive(Debug)]
pub enum DecisionTree {
    Leaf {
        branch_idx: usize,
        bindings: Vec<(MIRLocalID, MIRPlace)>,
    },
    Switch {
        place: MIRPlace,
        cases: Vec<(Constructor, Self)>,
        default: Option<Box<Self>>,
    },
    Fail,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum Constructor {
    Variant(usize),
    IntLit(i128),
    BoolLit(bool),
    // TODO: maybe add more
}

pub struct Matrix<'a> {
    pub cols: Vec<MIRPlace>,
    pub rows: Vec<Row<'a>>,
}

pub struct Row<'a> {
    pub pats: Vec<Option<&'a ThirPattern>>,
    pub branch_idx: usize,
    pub bindings: Vec<(MIRLocalID, MIRPlace)>,
}

impl<'a> Matrix<'a> {
    pub fn compile(self, ctx: &mut ThirToMIR<'_>) -> DecisionTree {
        let Some(fst) = self.rows.first() else {
            return DecisionTree::Fail;
        };
        if fst
            .pats
            .iter()
            .all(|p| p.is_none_or(|pat| pat.is_wildcard_like()))
        {
            return DecisionTree::Leaf {
                branch_idx: fst.branch_idx,
                bindings: self.collect_bindings(fst, ctx),
            };
        }
        let col = self.choose_col();
        let sig = self.collect_constructors(col);

        let cases = sig
            .iter()
            .map(|ctor| {
                let specialized = self.specialize(col, *ctor, ctx);
                (*ctor, specialized.compile(ctx))
            })
            .collect_vec();

        let place = self.cols[col].clone();
        let ty = place.ty.peeled(ctx.db);

        if ty.as_tuple_ref(ctx.db).is_some() || ty.as_struct_ref(ctx.db).is_some() {
            todo!("expand irrefutable pattern")
        }

        let default = if is_complete(ctx.db, &sig, ty) {
            None
        } else {
            Some(Box::new(self.default(col, ctx).compile(ctx)))
        };

        DecisionTree::Switch {
            place,
            cases,
            default,
        }
    }

    fn default(&self, col: usize, ctx: &mut ThirToMIR) -> Self {
        let mut cols = self.cols.clone();
        cols.remove(col);

        let mut rows = vec![];

        for row in &self.rows {
            let pat = row.pats[col];
            if pat.is_none_or(|pat| pat.is_wildcard_like()) {
                let mut new_pats = row.pats.clone();
                new_pats.remove(col);
                let mut bindings = row.bindings.clone();
                if let Some(pat) = pat
                    && let ThirPatternKind::Bind { local, .. } = &pat.kind
                {
                    bindings.push((ctx.local_map[local], self.cols[col].clone()));
                }
                rows.push(Row {
                    pats: new_pats,
                    branch_idx: row.branch_idx,
                    bindings,
                });
            }
        }
        Self { cols, rows }
    }

    fn specialize(&self, col: usize, ctor: Constructor, ctx: &mut ThirToMIR) -> Self {
        let arity = self.arity_of(col, ctor, ctx);
        let new_places = self.subplaces_for(col, ctor, &arity, ctx);
        let mut cols = self.cols.clone();
        cols.splice(col..=col, new_places.clone());
        let mut rows = vec![];

        for row in &self.rows {
            let pat = row.pats[col];
            if let Some(pat) = pat
                && let Some(pat_ctor) = pat.constructor()
                && pat_ctor == ctor
            {
                let sub_pats = pat.sub_patterns_for(&arity);
                let mut new_pats = row.pats.clone();
                new_pats.splice(col..=col, sub_pats);
                rows.push(Row {
                    pats: new_pats,
                    branch_idx: row.branch_idx,
                    bindings: row.bindings.clone(),
                });
            } else if pat.is_none_or(|pat| pat.is_wildcard_like()) {
                let mut new_pats = row.pats.clone();
                let fresh_wildcards =
                    std::iter::repeat(None).take(arity.len()).collect_vec();
                new_pats.splice(col..=col, fresh_wildcards);
                let mut bindings = row.bindings.clone();
                match pat {
                    Some(ThirPattern {
                        kind: ThirPatternKind::Bind { local, .. },
                        ..
                    }) => {
                        bindings.push((ctx.local_map[local], self.cols[col].clone()));
                    }

                    _ => (),
                }
                rows.push(Row {
                    pats: new_pats,
                    branch_idx: row.branch_idx,
                    bindings,
                });
            }
            // else: drop row
        }

        Self { cols, rows }
    }

    fn subplaces_for(
        &self,
        col: usize,
        ctor: Constructor,
        arity: &VariantArity,
        ctx: &mut ThirToMIR,
    ) -> Vec<MIRPlace> {
        let base = &self.cols[col];
        match arity {
            VariantArity::None => vec![],
            VariantArity::Tuple(n) => {
                let Constructor::Variant(variant_idx) = ctor else {
                    panic!("Only variant constructors are supported here")
                };
                (0..*n)
                    .map(|i| ctx.project_downcast_tuple_field(base, variant_idx, i))
                    .collect()
            }
            VariantArity::Struct(symbols) => {
                let Constructor::Variant(variant_idx) = ctor else {
                    panic!("Only variant constructors are supported here")
                };
                symbols
                    .iter()
                    .map(|field| ctx.project_downcast_field(base, variant_idx, *field))
                    .collect()
            }
        }
    }

    fn arity_of(
        &self,
        col: usize,
        ctor: Constructor,
        ctx: &mut ThirToMIR,
    ) -> VariantArity {
        let ty = self.cols[col].ty.peeled(ctx.db);
        match ctor {
            Constructor::Variant(idx) => {
                let id = ty.as_enum_ref(ctx.db).expect("This should be an enum").def;
                let kind = &enum_item(ctx.db, id.interned()).variants[idx].kind;
                match kind {
                    AstEnumVariantKind::Unit => VariantArity::None,
                    AstEnumVariantKind::StructLike(fields) => VariantArity::Struct(
                        fields.iter().map(|field| field.name).unique().collect(),
                    ),
                    AstEnumVariantKind::TupleLike(tys) => VariantArity::Tuple(tys.len()),
                }
            }
            Constructor::BoolLit(_) | Constructor::IntLit(_) => VariantArity::None,
        }
    }

    fn choose_col(&self) -> usize {
        assert!(!self.cols.is_empty());
        for i in 0..self.cols.len() {
            if let Some(row) = self.rows.first()
                && !row.pats[i].is_none_or(|pat| pat.is_wildcard_like())
            {
                return i;
            }
        }
        0
    }

    fn collect_constructors(&self, col: usize) -> HashSet<Constructor> {
        let mut res: HashSet<Constructor> = HashSet::new();
        for row in &self.rows {
            let pat = &row.pats[col];
            if let Some(pat) = pat
                && let Some(ctor) = pat.constructor()
            {
                res.insert(ctor);
            }
        }
        res
    }

    fn collect_bindings(
        &self,
        row: &Row<'a>,
        ctx: &ThirToMIR<'_>,
    ) -> Vec<(MIRLocalID, MIRPlace)> {
        let mut bindings = row.bindings.clone();
        for (pat, place) in row.pats.iter().zip_eq(&self.cols) {
            match pat {
                Some(ThirPattern {
                    kind: ThirPatternKind::Bind { local, .. },
                    ..
                }) => {
                    bindings.push((ctx.local_map[local], place.clone()));
                }
                Some(ThirPattern {
                    kind: ThirPatternKind::Any,
                    ..
                }) => (),
                Some(_) => unreachable!(),
                None => (), // synthetic wildcard
            }
        }
        bindings
    }
}

impl ThirPattern {
    pub fn is_wildcard_like(&self) -> bool {
        match &self.kind {
            ThirPatternKind::Bind { .. } | ThirPatternKind::Any => true,
            _ => false,
        }
    }

    fn constructor(&self) -> Option<Constructor> {
        match &self.kind {
            ThirPatternKind::Constructor { idx, .. } => Some(Constructor::Variant(*idx)),
            ThirPatternKind::IntLit(val) => Some(Constructor::IntLit(*val as i128)),
            _ => None,
        }
    }

    fn sub_patterns_for(&self, arity: &VariantArity) -> Vec<Option<&ThirPattern>> {
        match &self.kind {
            ThirPatternKind::Constructor { args, .. } => match (args, arity) {
                (ThirConstructorArgs::Tuple(items), VariantArity::Tuple(n)) => {
                    assert_eq!(items.len(), *n);
                    items.iter().map(Some).collect()
                }
                (ThirConstructorArgs::Struct(items), VariantArity::Struct(symbs)) => {
                    symbs
                        .iter()
                        .map(|symb| {
                            items.iter().find(|(name, _)| symb == name).map(|f| &f.1)
                        })
                        .collect()
                }
                (ThirConstructorArgs::None, VariantArity::None) => vec![],
                _ => unreachable!(),
            },
            ThirPatternKind::Tuple(_) | ThirPatternKind::Struct { .. } => unreachable!(),
            _ => std::iter::repeat(None).take(arity.len()).collect_vec(),
        }
    }
}

fn is_complete(db: &dyn Db, sig: &HashSet<Constructor>, ty: TypeRef) -> bool {
    let sig: HashSet<Constructor> = HashSet::from_iter(sig.iter().copied());
    match ty {
        TypeRef::Concrete(_) if let Some(enum_ref) = ty.as_enum_ref(db) => {
            let enum_id = enum_ref.def;
            let variant_count = enum_item(db, enum_id.into()).variants.len();
            variant_count == sig.len()
        }
        TypeRef::Concrete(id) if id == bool_id(db) => {
            sig.contains(&Constructor::BoolLit(true))
                && sig.contains(&Constructor::BoolLit(false))
        }
        TypeRef::Concrete(_)
            if ty.as_tuple_ref(db).is_some() || ty.as_struct_ref(db).is_some() =>
        {
            true
        }
        TypeRef::Concrete(_) => todo!(),
        _ => false,
    }
}

enum VariantArity {
    None,
    Tuple(usize),
    Struct(Vec<Symbol>),
}

impl VariantArity {
    pub fn len(&self) -> usize {
        match self {
            VariantArity::None => 0,
            VariantArity::Tuple(n) => *n,
            VariantArity::Struct(symbols) => symbols.len(),
        }
    }
}

impl<'a> ThirToMIR<'a> {
    pub fn project_downcast_tuple_field(
        &mut self,
        base: &MIRPlace,
        variant_idx: usize,
        tuple_idx: usize,
    ) -> MIRPlace {
        let mut projections = base.projections.clone();
        projections.push(MIRProjection::Downcast {
            variant: variant_idx,
        });
        let ConstructorType::Tuple(tys) = base
            .ty
            .as_enum_ref(self.db)
            .expect("Should we peel it ?")
            .get_cons(self.db, variant_idx)
            .unwrap()
        else {
            panic!("Expected tuple constructor");
        };
        let resulting_ty = tys[tuple_idx];

        projections.push(MIRProjection::TupleField {
            index: tuple_idx as u32,
            resulting_ty,
        });
        MIRPlace {
            local: base.local,
            projections,
            ty: resulting_ty,
        }
    }

    pub fn project_downcast_field(
        &mut self,
        base: &MIRPlace,
        variant_idx: usize,
        field: Symbol,
    ) -> MIRPlace {
        let mut projections = base.projections.clone();
        projections.push(MIRProjection::Downcast {
            variant: variant_idx,
        });
        let ConstructorType::Struct(tys) = base
            .ty
            .as_enum_ref(self.db)
            .expect("Should we peel it ?")
            .get_cons(self.db, variant_idx)
            .unwrap()
        else {
            panic!("Expected tuple constructor");
        };
        let resulting_ty = tys.iter().find(|(name, _)| *name == field).unwrap().1;

        projections.push(MIRProjection::Field {
            name: field,
            resulting_ty,
        });
        MIRPlace {
            local: base.local,
            projections,
            ty: resulting_ty,
        }
    }
}
