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

use crate::{
    Db,
    check::{
        check, collect_module_functions, fundef::reachable_mir_instances, reachable_frefs,
    },
    codegen::{Codegen, MIRToLIRBuild, MIRToLIRDeclare},
    common::location::LocationInfo,
    compiler::diagnostic::{Diag, Severity},
    compiler::timing::{PhaseStat, render_timings},
    mir::passes::dead_code_elimination::dce,
    name_resolve::file_module_id,
    printer::render_diagnostics,
    resolved::{FunctionId, TypeRef},
    thir::{ExprKind, thir_body},
    thir_to_mir::{MIRKey, mir},
};
use inkwell::{
    OptimizationLevel,
    context::Context,
    module::Module,
    passes::PassBuilderOptions,
    targets::{
        CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
    },
};
use itertools::Itertools;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub mod diagnostic;
mod load;
pub mod timing;
mod workspace;

pub use load::*;
pub use workspace::*;

pub fn program_has_errors(db: &dyn Db) -> (bool, Vec<&Diag>) {
    let ws = Workspace::get(db);
    let raw_diags: Vec<&Diag> = check::accumulated::<Diag>(db, ws);
    let has_errors = raw_diags.iter().any(|d| d.severity == Severity::Error);
    (has_errors, raw_diags)
}

pub fn check_from_disk(root: PathBuf, config: Config) -> Result<(), CompilerError> {
    let db = load_workspace_from_disk(root, config)?;
    let ws = Workspace::get(&db);
    check(&db, ws);
    let (has_errors, raw_diags) = program_has_errors(&db);

    let mut diags: Vec<(&LocationInfo, &Diag)> = raw_diags
        .into_iter()
        .map(|d| (d.primary.span.start().loc_info(&db), d))
        .collect();

    diags.sort_by(|(loc_a, diag_a), (loc_b, diag_b)| {
        diag_a.severity.cmp(&diag_b.severity).then_with(|| {
            loc_a.cmp(loc_b).then_with(|| diag_a.message.cmp(&diag_b.message))
        })
    });

    render_diagnostics(&db, diags.into_iter().map(|(_, diag)| diag));
    if has_errors { Err(CompilerError::CompiledWithErrors) } else { Ok(()) }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct FrefSortKey {
    info: LocationInfo,
    subs: Vec<String>,
}

pub fn build<'db, 'ctx>(
    db: &'db dyn Db,
    w: Workspace,
    mut c: Codegen<'db, MIRToLIRDeclare<'db, 'ctx>>,
) -> Codegen<'db, MIRToLIRBuild<'db, 'ctx>> {
    let packages = workspace_packages(db, w);
    let frefs = packages
        .iter()
        .flat_map(|pkg| reachable_frefs(db, *pkg).iter().copied().collect_vec())
        .unique()
        .collect_vec();

    for fref in frefs {
        if fref.fdef(db).has_body(db) {
            let dce = dce(db, fref);
            c.declare_mir(dce);
        } else {
            c.declare_import(
                fref.fdef(db).name(db).to_string(db),
                &fref.params(db).iter().map(|(_, ty)| *ty).collect_vec(),
                fref.ret_ty(db),
                fref,
            );
        }
    }

    c.finalize()
}

fn thir_is_lowerable(db: &dyn Db, fdef: FunctionId) -> bool {
    thir_body(db, fdef).is_some_and(|thir| {
        thir.exprs.iter().all(|(_, expr)| !matches!(expr.kind, ExprKind::Error))
    })
}

fn display_ir(db: &dyn Db, w: Workspace) {
    let cfg = db.config();
    if !cfg.display_thir && !cfg.display_mir {
        return;
    }

    let packages = workspace_packages(db, w);
    let mut seen: HashSet<MIRKey> = HashSet::new();
    let mut instances: Vec<(FunctionId, Vec<TypeRef>)> = vec![];
    for pkg in packages {
        let module = file_module_id(db, *pkg.root(db), None, *pkg);
        for root in collect_module_functions(db, module.interned()) {
            for (fdef, subs) in reachable_mir_instances(db, root) {
                if seen.insert(MIRKey::new(db, fdef, subs.clone())) {
                    instances.push((fdef, subs));
                }
            }
        }
    }

    if cfg.display_thir {
        let mut fids = instances
            .iter()
            .map(|(fdef, _)| *fdef)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect_vec();
        fids.sort_by_key(|id| id.span(db).start().loc_info(db).clone());
        for fid in fids {
            if let Some(thir) = thir_body(db, fid) {
                println!("{}", thir.display(db));
            }
        }
    }

    if cfg.display_mir {
        let mut instances = instances;
        instances.sort_by_key(|(fdef, subs)| FrefSortKey {
            info: fdef.name_span(db).start().loc_info(db).clone(),
            subs: subs.iter().map(|ty| ty.to_string(db)).collect(),
        });
        for (fdef, subs) in instances {
            if !fdef.has_body(db) {
                continue;
            }
            if !thir_is_lowerable(db, fdef) {
                continue;
            }
            let the_mir = mir(db, fdef, subs.clone());
            let dce = dce(db, the_mir.func);
            println!("{}", fdef.called_to_string(db));
            println!("{}\n", dce.display(db));
        }
    }
}

pub fn write_object_file(
    m: &Module<'_>,
    path: &Path,
    machine: &TargetMachine,
) -> Result<(), String> {
    machine.write_to_file(m, FileType::Object, path).map_err(|e| e.to_string())
}

fn link_executable(obj: &Path, out: &Path) -> Result<(), CompilerError> {
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let status =
        std::process::Command::new(&cc).arg(obj).arg("-o").arg(out).status().map_err(
            |e| CompilerError::LinkFailed(format!("failed to spawn `{cc}`: {e}")),
        )?;
    if !status.success() {
        return Err(CompilerError::LinkFailed(format!("`{cc}` exited with {status}")));
    }
    Ok(())
}

impl OptLevel {
    fn to_llvm(self) -> OptimizationLevel {
        match self {
            Self::O0 => OptimizationLevel::None,
            Self::O1 => OptimizationLevel::Less,
            Self::O2 => OptimizationLevel::Default,
            Self::O3 => OptimizationLevel::Aggressive,
        }
    }
}

pub fn optimize(module: &Module, opt_level: OptimizationLevel) -> TargetMachine {
    module.verify().unwrap();

    Target::initialize_native(&InitializationConfig::default())
        .expect("failed to initialize native target");

    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).unwrap();
    let machine = target
        .create_target_machine(
            &triple,
            TargetMachine::get_host_cpu_name().to_str().unwrap(),
            TargetMachine::get_host_cpu_features().to_str().unwrap(),
            opt_level,
            RelocMode::PIC,
            CodeModel::Small,
        )
        .unwrap();

    // Do this BEFORE running passes: without a data layout on the module,
    // several passes make wrong or conservative decisions.
    module.set_triple(&triple);
    module.set_data_layout(&machine.get_target_data().get_data_layout());

    let passes = match opt_level {
        OptimizationLevel::None => "default<O0>",
        OptimizationLevel::Less => "default<O1>",
        OptimizationLevel::Default => "default<O2>",
        OptimizationLevel::Aggressive => "default<O3>",
    };

    module
        .run_passes(passes, &machine, PassBuilderOptions::create())
        .expect("optimization passes failed");

    machine
}

pub fn build_from_disk(root: PathBuf, config: Config) -> Result<(), CompilerError> {
    let stem = root
        .is_file()
        .then(|| root.file_stem().and_then(|s| s.to_str()).map(String::from))
        .flatten();
    let db = load_workspace_from_disk(root, config)?;
    let ws = Workspace::get(&db);
    let overall = Instant::now();
    check(&db, ws);
    let (has_errors, raw_diags) = program_has_errors(&db);

    let mut diags: Vec<(&LocationInfo, &Diag)> = raw_diags
        .into_iter()
        .map(|d| (d.primary.span.start().loc_info(&db), d))
        .collect();

    diags.sort_by(|(loc_a, diag_a), (loc_b, diag_b)| {
        diag_a.severity.cmp(&diag_b.severity).then_with(|| {
            loc_a.cmp(loc_b).then_with(|| diag_a.message.cmp(&diag_b.message))
        })
    });

    render_diagnostics(&db, diags.into_iter().map(|(_, diag)| diag));

    display_ir(&db, ws);

    let report = |llvm: Option<Duration>| {
        if db.config().show_time {
            render_timings(
                &check::accumulated::<PhaseStat>(&db, ws),
                llvm,
                overall.elapsed(),
            );
        }
    };

    if has_errors {
        report(None);
        return Err(CompilerError::CompiledWithErrors);
    }

    let t_llvm = Instant::now();
    let llvm_ctx = Context::create();
    let cg = Codegen::<MIRToLIRDeclare>::new(&db);

    // Collect MIR
    let cg = build(&db, ws, cg);

    // MIR -> LIR
    let cg = cg.run();

    // LIR -> LLVM
    let llvm_module = cg.finalize(&llvm_ctx);

    if db.config().display_llvm {
        llvm_module.print_to_stderr();
    }

    let machine = optimize(&llvm_module, db.config().opt_level.to_llvm());

    if db.config().display_opt_llvm {
        llvm_module.print_to_stderr();
    }
    let llvm_wall = t_llvm.elapsed();

    let write_object = |path: &Path| {
        write_object_file(&llvm_module, path, &machine).map_err(|err| {
            eprintln!("LLVM errors:\n{err}");
            CompilerError::CompiledWithErrors
        })
    };

    if db.config().compile_only {
        let obj_path = db.config().output.clone().unwrap_or_else(|| {
            PathBuf::from(format!("{}.o", stem.as_deref().unwrap_or("a")))
        });
        write_object(&obj_path)?;
        report(Some(llvm_wall));
        return Ok(());
    }

    let exe_path = db
        .config()
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from(stem.as_deref().unwrap_or("a.out")));
    let obj_path =
        std::env::temp_dir().join(format!("{}.o", stem.as_deref().unwrap_or("a")));

    write_object(&obj_path)?;
    let link_result = link_executable(&obj_path, &exe_path);
    let _ = std::fs::remove_file(&obj_path);
    link_result?;
    report(Some(llvm_wall));
    Ok(())
}
