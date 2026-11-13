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

use rockr_base::{
    Db, Span,
    arena::{Arena, Idx},
    mutability::Mutability,
    symbol::Symbol,
    workspace::Workspace,
};

use crate::{
    hir::{self, signature::get_sig_of_function},
    mir::{
        basic_block::{MIRBasicBlock, MIRTerminator},
        cache::MIRCache,
        operand::{
            MIRCallee, MIRConstant, MIRConstructorArgs, MIROperand, MIRPlace,
            MIRProjection, MIRRValue, MIRRValueKind,
        },
    },
    model::{
        FunctionId, ScopeOwnerId, TypeRef,
        concrete::{ConcreteTy, Substitution},
        packages::workspace_packages,
    },
    name_resolve::{
        core_package, file_module_id, std_package, type_expr::get_templates_of_fun,
    },
    thir::{self, FunctionRef},
};
use std::{
    collections::{HashMap, HashSet},
    iter::{empty, once},
    marker::PhantomData,
};

pub mod analysis;
pub mod basic_block;
pub mod builder;
pub mod cache;
pub mod display;
pub mod operand;
pub mod passes;

// MIR-local type aliases
type BasicBlock = MIRBasicBlock;
type BlockID = MIRBlockID;
type Local = MIRLocal;
type LocalID = MIRLocalID;
type Terminator = MIRTerminator;
type Operand = MIROperand;
type Place = MIRPlace;
type Constant = MIRConstant;
type Projection = MIRProjection;
type RValue = MIRRValue;
type RValueKind = MIRRValueKind;
type Callee = MIRCallee;
type ConstructorArgs = MIRConstructorArgs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MIRBlockID(pub Idx<BasicBlock>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MIRLocalID(pub Idx<Local>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyntacticSource {
    pub span: Span,
    pub id: hir::LocalId,
}

pub struct Mir {
    pub func: FuncInst,
    pub blocks: Arena<BasicBlock>,
    pub locals: Arena<Local>,
    pub parameters: Vec<LocalID>,
    pub entry: BlockID,

    cache: MIRCache,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MIRLocal {
    pub ty: ConcreteTy,
    pub mutability: Mutability,

    // Metadata:
    pub span: Span,
    pub name: Option<Symbol>,
    pub thir_src: Option<thir::LocalId>,
    pub syn_src: Option<SyntacticSource>,
}

impl MIRLocal {
    pub fn new(ty: ConcreteTy, mutability: Mutability, span: Span) -> Self {
        Self { ty, mutability, span, name: None, thir_src: None, syn_src: None }
    }

    #[must_use]
    pub fn with_thir_src(self, src: thir::LocalId) -> Self {
        let mut this = self;
        this.thir_src = Some(src);
        this
    }

    #[must_use]
    pub fn with_name(self, name: Symbol) -> Self {
        let mut this = self;
        this.name = Some(name);
        this
    }

    #[must_use]
    pub fn with_syn_src(self, src: SyntacticSource) -> Self {
        let mut this = self;
        this.syn_src = Some(src);
        this
    }
}

impl Mir {
    fn compute_reachable_aux(&self, s: &mut HashSet<BlockID>, blk: BlockID) {
        if !s.insert(blk) {
            return;
        }

        match &self.blocks[blk.0].terminator {
            MIRTerminator::Return { .. } | MIRTerminator::Diverge => (),
            MIRTerminator::Goto { next } | MIRTerminator::Call { next, .. } => {
                self.compute_reachable_aux(s, *next);
            }
            MIRTerminator::Branch { then, else_, .. } => {
                self.compute_reachable_aux(s, *then);
                self.compute_reachable_aux(s, *else_);
            }
            MIRTerminator::Switch { branches, default, .. } => {
                branches
                    .iter()
                    .map(|b| b.1)
                    .chain(once(default))
                    .for_each(|next| self.compute_reachable_aux(s, *next));
            }
        }
    }

    pub fn compute_reachable(&self, from: BlockID) -> HashSet<BlockID> {
        let mut res = HashSet::new();
        self.compute_reachable_aux(&mut res, from);
        res
    }

    fn compute_successors(&self) -> HashMap<BlockID, HashSet<BlockID>> {
        self.blocks
            .iter()
            .map(|(blk, infos)| {
                let succs = match &infos.terminator {
                    MIRTerminator::Return { .. } | MIRTerminator::Diverge => {
                        HashSet::new()
                    }
                    MIRTerminator::Goto { next } | MIRTerminator::Call { next, .. } => {
                        HashSet::from([*next])
                    }
                    MIRTerminator::Branch { then, else_, .. } => {
                        HashSet::from([*then, *else_])
                    }
                    MIRTerminator::Switch { branches, default, .. } => {
                        branches.iter().map(|b| *b.1).chain([*default]).collect()
                    }
                };
                (MIRBlockID(blk), succs)
            })
            .collect()
    }

    fn compute_predecessors(&self) -> HashMap<BlockID, HashSet<BlockID>> {
        let mut res: HashMap<BlockID, HashSet<BlockID>> = HashMap::new();
        self.successors().iter().for_each(|(pred, succs)| {
            for succ in succs {
                res.entry(*succ).or_default().insert(*pred);
            }
        });
        self.blocks.iter().for_each(|(blk, _)| {
            res.entry(MIRBlockID(blk)).or_default();
        });
        res
    }
}

impl PartialEq for Mir {
    fn eq(&self, other: &Self) -> bool {
        self.blocks == other.blocks
            && self.locals == other.locals
            && self.parameters == other.parameters
            && self.entry == other.entry
    }
}

impl Eq for Mir {}

#[salsa::interned]
pub struct MIRKey {
    pub fdef: FunctionId,
    pub subs: Substitution,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncInst(pub salsa::Id);

impl<'db> From<MIRKey<'db>> for FuncInst {
    fn from(value: MIRKey<'db>) -> Self {
        Self(value.0)
    }
}

impl FuncInst {
    pub fn interned<'db>(self) -> MIRKey<'db> {
        MIRKey(self.0, PhantomData)
    }

    pub fn fdef(self, db: &dyn Db) -> FunctionId {
        *self.interned().fdef(db)
    }

    pub fn subs(self, db: &dyn Db) -> &Substitution {
        self.interned().subs(db)
    }

    pub fn from_funcref(db: &dyn Db, fref: &FunctionRef) -> Self {
        assert_eq!(get_templates_of_fun(db, fref.id.interned()).len(), fref.args.len());
        let empty_sub = Substitution::new(empty());
        let concrete_tys = fref.args.iter().map(|ty| {
            ty.to_concrete(db, &empty_sub)
                .expect("If the fref isn't known to be concrete, this is a bug")
        });
        MIRKey::new(db, fref.id, Substitution::new(concrete_tys)).into()
    }

    pub fn ret_ty(self, db: &dyn Db) -> ConcreteTy {
        let sig = get_sig_of_function(db, self.fdef(db).interned());
        let ret_ty = match sig.ret {
            TypeRef::Zelf => self.fdef(db).parent(db).get_canonical_zelf(db).unwrap(),
            ret => ret,
        };
        ret_ty.to_concrete(db, self.subs(db)).unwrap()
    }

    pub fn params(self, db: &dyn Db) -> Vec<(Symbol, ConcreteTy)> {
        let sig = get_sig_of_function(db, self.fdef(db).interned());
        let fdef = self.fdef(db);
        let zelf = fdef.parent(db).get_canonical_zelf(db);
        let receiver = sig.zelf.map(|arg| {
            (
                Symbol::new(db, "self"),
                arg.as_type_ref_for(db, zelf.unwrap())
                    .to_concrete(db, self.subs(db))
                    .unwrap(),
            )
        });
        receiver
            .into_iter()
            .chain(
                sig.args.iter().map(|(symb, ty)| {
                    (*symb, ty.to_concrete(db, self.subs(db)).unwrap())
                }),
            )
            .collect()
    }
}

#[salsa::tracked]
impl FuncInst {
    pub fn is_variadic(self, db: &dyn Db) -> bool {
        self.fdef(db).is_var_args(db)
    }

    pub fn is_main(self, db: &dyn Db) -> bool {
        let packages = workspace_packages(db, Workspace::get(db));
        let mut main_pkg = None;
        for pkg in packages {
            if Some(*pkg) == std_package(db) {
                continue;
            }
            if *pkg == core_package(db) {
                continue;
            }
            assert!(main_pkg.is_none(), "TODO: handle multiple packages for main");

            main_pkg = Some(*pkg);
        }
        let Some(main_pkg) = main_pkg else {
            return false;
        };
        let file_module = file_module_id(db, *main_pkg.root(db), None, main_pkg);
        let f_id = FunctionId::new(
            db,
            Symbol::new(db, "main"),
            ScopeOwnerId::Module(file_module),
        );
        if self.fdef(db) != f_id {
            return false;
        }
        assert!(self.subs(db).is_empty(), "Generic main function !");

        true
    }
}
