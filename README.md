# Rockr Programming Language

Rockr is a work in progress compiler for a Rust-like language with meta-programming in mind.

----------
⚠️ **Rockr is in very early development**
----------

## Quick start
```sh
git clone https://codeberg.org/norezap/Rockr.git
cd Rockr
cargo run -- examples/hello.rkr --skip-core
```

## Syntax
The syntax is very prone to change and evolve but shouldn't be hard to pick up if you know some basic Rust, C or C++.
```rkr
fun main(): int {
    "Hello, World !".println();
    return 0;
}
```

## What works today
Rockr already goes all the way from source to a native binary through LLVM:

- Type inference and checking, with generics and an interface system
- Pattern matching and destructuring
- Borrow checking (partial moves, mutable borrows) and drop
- MIR-based analyses (borrow/loan checking, all-paths-return)
- Codegen through its own SSA IR (MIR → LIR → LLVM)
- A language server (LSP), with a WIP Zed extension

See `TODO.md` for the roadmap and where things are headed.

## Fun stuff
You can do (what I consider) some pretty fun stuff in Rockr. Here's a
compile-time state machine: the type carries the state, so illegal transitions
just don't type-check.
```rkr
interface State {}
struct Locked {}
struct Open {}
impl State for Locked {}
impl State for Open {}

struct Door<S: State> {}

impl Door<Locked> {
  fun new(): Self { return Self { ._s: PhantomData {} }; }
  fun unlock(self): Door<Open> { return Door { ._s: PhantomData {} }; }
}

impl Door<Open> {
  fun lock(self): Door<Locked> { return Door { ._s: PhantomData {} }; }
}

fun main(): int {
  let door = Door<Locked>::new();
  let door = door.unlock(); // now Door<Open>; a second .unlock() would not compile
  @type_name(Door<Open>).println(); // Outputs `Door<Open>`
  return 0;
}
```

## Contributing
All contributions are welcome! Just submit a PR or message me if interested.
