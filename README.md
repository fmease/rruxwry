# rrustdoc – A rustdoc wrapper for rustdoc devs

A power tool for rustdoc devs that wraps `rustdoc` and `rustc`.

<!-- FIXME: Mention ui_test support -->

Its most useful features are the flags `-x`/`--cross-crate` and `-o`/`--open`.
For reproducing or debugging a [rustdoc *cross-crate re-exports* issue](https://github.com/rust-lang/rust/labels/A-cross-crate-reexports), `rrustdoc` reduces the number of steps from many to just two:

1. Creating a single file `file.rs`,
2. Executing `rrustdoc file.rs -xo`.

The alternatives would be:

* Creating at least two files, running `rustc` then `rustdoc` manually without forgetting the various flags that need to be passed and manually opening up the generated documentation.
* Setting up a Cargo project containing at least two crates and running `cargo doc --open`.

## Tutorial

TODO

## Command-line interface

`rrustdoc -h`:

```
A rustdoc wrapper for rustdoc devs

Usage: rrustdoc [OPTIONS] <PATH>

Arguments:
  <PATH>  Path to the source file

Options:
  -o, --open                     Open the generated docs in a browser
  -n, --crate-name <NAME>        Set the name of the (base) crate
  -y, --crate-type <TYPE>        Set the type of the (base) crate
  -e, --edition <EDITION>        Set the edition of the source files [possible values: 2015, 2018, 2021, 2024]
  -t, --toolchain <NAME>         Set the toolchain
      --cfg <SPEC>               Enable a `cfg`
  -f, --cargo-feature <NAME>     Enable a Cargo-like feature
  -F, --rustc-feature <NAME>     Enable an experimental rustc library or language feature
  -j, --json                     Output JSON instead of HTML
  -v, --crate-version <VERSION>  Set the version of the (root) crate
  -P, --private                  Document private items
  -H, --hidden                   Document hidden items
      --layout                   Document the memory layout of types
  -D, --link-to-definition       Generate links to definitions
      --normalize                Normalize types and constants
      --theme <THEME>            Set the theme [default: ayu]
      --cap-lints <LEVEL>        Cap lints at a level [possible values: allow, warn, deny, forbid]
  -#, --internals                Enable rustc's `-Zverbose-internals`
      --log                      Override `RUSTC_LOG` to be `debug`
  -B, --no-backtrace             Override `RUST_BACKTRACE` to be `0`
  -X, --cross-crate              Enable the cross-crate re-export mode
  -T, --compiletest              Enable ui_test-style compiletest directives: `//@`
  -Q, --query                    Enable XPath / JsonPath queries
  -V, --verbose                  Use verbose output
  -0, --dry-run                  Run through without making any changes
      --color <WHEN>             Control when to use color [default: auto] [possible values: auto, always, never]
  -h, --help                     Print help
```

## License

Except as otherwise noted, the contents of this repository are licensed under the MIT license (see the [license file][./LICENSE]). Some files include or are accompanied by explicit license notices.
