# rrustdoc – A rustdoc wrapper for rustdoc devs

A power tool for rustdoc devs that wraps `rustdoc` and `rustc`.

Its most useful features are the flags `-x`/`--cross-crate` and `-o`/`--open`.
For reproducing or debugging a [rustdoc *cross-crate re-exports* issue](https://github.com/rust-lang/rust/labels/A-cross-crate-reexports), `rrustdoc` reduces the number of steps from many to just two:

1. Creating a single file `file.rs`,
2. Executing `rrustdoc file.rs -xo`.

The alternatives would be:

* Creating at least two files, running `rustc` then `rustdoc` manually without forgetting the various flags that need to be passed and manually opening up the generated documentation.
* Setting up a Cargo project containing at least two crates and running `cargo doc --open`.

`rrustdoc -h`:

```
A rustdoc wrapper for rustdoc devs

Usage: rrustdoc [OPTIONS] <PATH>

Arguments:
  <PATH>  Path to the source file

Options:
  -e, --edition <EDITION>        Set the edition of the source files [default: 2021]
  -H, --hidden                   Document hidden items
  -j, --json                     Output JSON instead of HTML
  -L, --layout                   Document the memory layout of types
  -N, --normalize                Normalize types and constants
  -n, --crate-name <NAME>        Set the (base) name of the crate(s)
  -v, --crate-version <VERSION>  Set the version of the (root) crate
  -o, --open                     Open the generated docs in a browser
  -P, --private                  Document private items
  -a, --crate-name-attr          Pick up the crate name from `#![crate_name]` if available
      --cfg <SPEC>               Enable a `cfg`
  -f, --feature <NAME>           Enable a Cargo-like feature
  -F, --rustc-feature <NAME>     Enable an experimental rustc library or language feature
  -t, --toolchain <TOOLCHAIN>    Set the toolchain
  -V, --verbose                  Use verbose output
  -#, --internals                Enable rustc's `-Zverbose-internals`
  -l, --log                      Override `RUSTC_LOG` to be `debug`
  -B, --no-backtrace             Override `RUST_BACKTRACE` to be `0`
  -x, --cross-crate              Enable the cross-crate re-export mode
  -h, --help                     Print help
```
