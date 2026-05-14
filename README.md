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
Honestly, the syntax is very prone to change and evolve but shouldn't be hard to pick up if you know some basic Rust, C or C++.
```rkr
@include core::io

fun main(): int {
    "Hello, World !".println();
    return 0;
}
```

## Contributing
All contributions are welcome! Just submit a PR or message me if interested.
