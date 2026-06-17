use std::collections::HashSet;

use itertools::Itertools;

use crate::{
    Db,
    mir::{MIRLocalID, operand::MIRPlace},
    name_resolve::type_expr::enum_item,
    ril::{TypeRef, bool_id},
    thir::{ThirPattern, ThirPatternKind},
    thir_to_mir::ThirToMIR,
};

// Compiling pattern matching to decision tree, inspired by 
// http://moscova.inria.fr/~maranget/papers/ml05e-maranget.pdf
enum DecisionTree {
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
enum Constructor {
    Variant(usize),
    IntLit(i128),
    BoolLit(bool),
    // TODO: maybe add more
}

struct Matrix<'a> {
    cols: Vec<MIRPlace>,
    rows: Vec<Row<'a>>,
}

struct Row<'a> {
    pats: Vec<&'a ThirPattern>,
    branch_idx: usize,
    bindings: Vec<(MIRLocalID, MIRPlace)>,
}

impl<'a> Matrix<'a> {
    pub fn compile(self, ctx: &ThirToMIR<'_>) -> DecisionTree {
        let Some(fst) = self.rows.first() else {
            return DecisionTree::Fail;
        };
        if fst.pats.iter().all(|p| p.is_wildcard_like()) {
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
                let specialized = self.specialize(col, *ctor);
                (*ctor, specialized.compile(ctx))
            })
            .collect_vec();

        let place = self.cols[col].clone();
        let ty = place.ty.peeled(ctx.db);

        if ty.as_tuple_ref(ctx.db).is_some() || ty.as_struct_ref(ctx.db).is_some() {
            // Expand irrefutable patterns
            todo!()
        }

        let default = if is_complete(ctx.db, &sig, ty) {
            None
        } else {
            Some(Box::new(self.default(col).compile(ctx)))
        };

        DecisionTree::Switch {
            place,
            cases,
            default,
        }
    }

    fn specialize(&self, col: usize, ctor: Constructor) -> Self {
        todo!()
    }

    fn choose_col(&self) -> usize {
        assert!(!self.cols.is_empty());
        for i in 0..self.cols.len() {
            if let Some(row) = self.rows.first()
                && !row.pats[i].is_wildcard_like()
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
            if let Some(ctor) = pat.constructor() {
                res.insert(ctor);
            }
        }
        res
    }

    fn default(&self, col: usize) -> Self {
        todo!()
    }

    fn collect_bindings(
        &self,
        row: &Row<'a>,
        ctx: &ThirToMIR<'_>,
    ) -> Vec<(MIRLocalID, MIRPlace)> {
        let mut bindings = row.bindings.clone();
        for (pat, place) in row.pats.iter().zip_eq(&self.cols) {
            match &pat.kind {
                ThirPatternKind::Bind { local, .. } => {
                    bindings.push((ctx.local_map[local], place.clone()));
                }
                ThirPatternKind::Any => (),
                _ => unreachable!(),
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
