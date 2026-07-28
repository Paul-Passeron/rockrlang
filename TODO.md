# TODO for Rockr

This tracks where the compiler is at. The **Todo** section can be a great
starting point for contributors looking for something to do.

## Done

### Parsing
- Hand-rolled recursive-descent / precedence-climbing parser (no generator)
- Turbofish for struct / enum literals, and made sure it's everywhere we want it
- Did some refactoring on the parser — it's in a better state than it used to be

### Type system & inference
- Create typed IR (THIR) from HIR + type inference
- Interface system: actually check that implementations implement the interface,
  and keep track of what types implement which interface at a program level
  (as a set of queries on concrete types)
- Keep track of implicit things the compiler should do: auto-deref, and
  function / method call informations
- Made speculative execution backtrack (with an undo log) rather than cloning
  the whole solver state

### MIR & checks
- Create mid-level IR from typed IR
- Run the borrow-checker on it, and made it smarter (partial move / mut borrow)
- Run path analysis (do all paths return, etc.)

### Codegen
- Figured out the backend: went with inkwell (LLVM)
- We wanted our own SSA IR before the backend — turns out we got two
  (MIR -> LIR -> LLVM)

### Diagnostics
- Display diagnostics
- Turn `ParseError`s into diagnostics

### Performance
- Made the fixed-point iteration general enough to run on bitsets (it takes a
  `bottom` closure now, so a lattice whose bottom depends on a runtime size
  can use it)
- Type inference backtracks with an undo log instead of deep-cloning its state
- Use bitsets in the MIR analyses: liveness (with uses/defs precomputed) and
  init / move tracking
- Keep use-after-move / init tracking in the interned key bitset domain instead
  of a `HashMap<MoveKey, _>` — this was the big one, borrow-checking used to
  dominate the whole compile
- Made dead-code elimination linear in blocks instead of cubic

### Other
- Removed `PartialTypeRef` and `PartialTypeArg` (`TypeRef::Unknown` does what
  they did)
- Made builtin types an enum instead of runtime salsa-interned structs

### Tooling
- Implemented the LSP
- Zed extension is WIP (highlighting isn't all there yet)

## Todo

### Diagnostics
- [ ] Handle constraint solver output gracefully. That might mean adding some
      metadata to the constraint, or a side-table that tracks it for them in a
      non-intrusive way
- [ ] Have the `ParseError` diagnostics look good
- [ ] A `diagnose_solver(unresolved, errors)` function to make good diagnostics
      out of solver error output

### Type system & inference
- [ ] Improve the type-checker
- [ ] Improve type inference, and the constraint solver — might want to factor
      it out as its own type
- [ ] Formalize the interface system a bit more
- [ ] A type-safe way of having concrete types instead of using `TypeRef` /
      `TypeId` everywhere, especially for MIR, to enforce that all types have
      been substituted

### Parsing
- [ ] Make the turbofish implementation resilient — a compiler warning wherever
      it isn't implemented yet
- [ ] See if we support unicode in files
- [ ] Improve match branch parsing (for the moment we force them to be `{ ... }`)
- [ ] Maybe rewrite the parser at one point, it's still a bit of a mess

### MIR & optimizations
- [ ] Maybe some language-specific optimizations?
  - [ ] Constant folding struct / tuple field accesses, so
        `Foo { .bar: 1, .baz: 2 }.baz` would just become `2`
  - [ ] Inlining too, so constant folding can work across function boundaries

### Codegen & LIR
- [ ] Finish verification of LIR (or start it, maybe)
- [ ] Make a pretty printer for LIR and add it as a display target
- [ ] Look into ZSTs spilling into LLVM — maybe it's not important, but doesn't
      hurt to look
- [ ] Went for inkwell, but look at other interesting backends (cranelift, ...)

### Performance
- [ ] Figure out the "Other (uninstrumented)" chunk — it's the biggest single
      piece of the release compile now, need to instrument it
- [ ] Move the rest of the MIR analyses (loans, path analysis) onto bitsets too,
      now that the fixed-point iteration can handle them
- [ ] `find_const` clones the whole unification table on every call
      (path-compression wants `&mut`); find a way around it

### Long-term
- [ ] Make it expr-based instead of stmt-based. The language is statement-based
      for the moment, more like C than Rust — it's simpler for now. I can see us
      moving to expressions once the architecture is a bit more mature
- [ ] Write a solid Zed extension (and other IDEs, but I use Zed)

### Ideas
- [ ] Make use of salsa's serialization to have a compilation cache, and look
      into incremental compilation
