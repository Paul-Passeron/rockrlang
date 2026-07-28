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
 
use std::cell::Cell;
use std::time::{Duration, Instant};

use salsa::Accumulator;

use crate::Db;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Parsing,
    NameResolution,
    TypeChecking,
    Thir,
    Monomorphization,
    Checking,
    BorrowCheck,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Self::Parsing => "Parsing",
            Self::NameResolution => "Name resolution",
            Self::TypeChecking => "Type checking",
            Self::Thir => "THIR lowering",
            Self::Monomorphization => "Monomorphization",
            Self::Checking => "Checking (discovery/validity)",
            Self::BorrowCheck => "Borrow check",
        }
    }

    pub const ORDER: [Self; 7] = [
        Self::Parsing,
        Self::NameResolution,
        Self::TypeChecking,
        Self::Thir,
        Self::Monomorphization,
        Self::Checking,
        Self::BorrowCheck,
    ];
}

#[salsa::accumulator]
#[derive(Debug, Clone)]
pub struct PhaseStat {
    pub phase: Phase,
    pub functions: usize,
    pub stmts: Option<usize>,
    pub exprs: Option<usize>,
    pub nanos: u128,
}

pub struct Counts {
    pub functions: usize,
    pub stmts: Option<usize>,
    pub exprs: Option<usize>,
}

thread_local! {
    static CHILD_NANOS: Cell<u128> = const { Cell::new(0) };
}

pub fn timed<T>(
    db: &dyn Db,
    phase: Phase,
    body: impl FnOnce() -> T,
    counts: impl FnOnce(&T) -> Counts,
) -> T {
    let parent_children = CHILD_NANOS.replace(0);
    let start = Instant::now();
    let result = body();
    let elapsed = start.elapsed().as_nanos();
    let my_children = CHILD_NANOS.get();
    let self_nanos = elapsed.saturating_sub(my_children);
    CHILD_NANOS.set(parent_children + elapsed);

    let c = counts(&result);
    PhaseStat {
        phase,
        functions: c.functions,
        stmts: c.stmts,
        exprs: c.exprs,
        nanos: self_nanos,
    }
    .accumulate(db);

    result
}

struct PhaseTotals {
    functions: usize,
    stmts: Option<usize>,
    exprs: Option<usize>,
    nanos: u128,
}

fn fmt_nanos(n: u128) -> String {
    #[allow(clippy::cast_precision_loss)]
    let f = n as f64;
    if n >= 1_000_000_000 {
        format!("{:.2}s", f / 1e9)
    } else if n >= 1_000_000 {
        format!("{:.2}ms", f / 1e6)
    } else if n >= 1_000 {
        format!("{:.2}\u{b5}s", f / 1e3)
    } else {
        format!("{n}ns")
    }
}

pub fn render_timings(stats: &[&PhaseStat], llvm: Option<Duration>, total: Duration) {
    eprintln!("=== Compilation timings ===");

    let mut accounted: u128 = 0;

    for phase in Phase::ORDER {
        let mut totals =
            PhaseTotals { functions: 0, stmts: None, exprs: None, nanos: 0 };
        for stat in stats.iter().filter(|s| s.phase == phase) {
            totals.functions += stat.functions;
            totals.nanos += stat.nanos;
            if let Some(n) = stat.stmts {
                totals.stmts = Some(totals.stmts.unwrap_or(0) + n);
            }
            if let Some(n) = stat.exprs {
                totals.exprs = Some(totals.exprs.unwrap_or(0) + n);
            }
        }
        accounted += totals.nanos;

        eprintln!("{}: took {}", phase.name(), fmt_nanos(totals.nanos));
        eprintln!("    {} functions processed.", totals.functions);
        if let Some(n) = totals.stmts {
            eprintln!("    {n} stmts processed.");
        }
        if let Some(n) = totals.exprs {
            eprintln!("    {n} exprs processed.");
        }
    }

    match llvm {
        Some(d) => {
            accounted += d.as_nanos();
            eprintln!("LLVM: took {} (no instrumentation yet)", fmt_nanos(d.as_nanos()));
        }
        None => eprintln!("LLVM: skipped"),
    }

    let other = total.as_nanos().saturating_sub(accounted);
    eprintln!("Other (uninstrumented): took {}", fmt_nanos(other));

    eprintln!("Total: took {}", fmt_nanos(total.as_nanos()));
}
