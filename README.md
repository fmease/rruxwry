# rrustdoc – A rustdoc wrapper for rustdoc devs

A power tool for rustdoc devs that wraps `rustdoc` and `rustc`.

Its most useful features are the flags `-o`/`--open`, `-X`/`--cross-crate` and `-T`/`--compiletest`.

## Power Features

### `--cross-crate`

Very useful for rapidly reproducing or debugging [rustdoc *cross-crate re-exports* issues][x-crate-reexport-bugs].
`rrustdoc` reduces the number of steps from many to just two:

1. Creating a single file `file.rs`,
2. Executing `rrustdoc file.rs -X` (or `rrustdoc file.rs -Xo`).

The alternatives would be:

* Creating at least two files, running `rustc` then `rustdoc` manually without forgetting the various flags that need to be passed and manually opening up the generated documentation with the browser of choice.
* Setting up a Cargo project containing at least two crates and running `cargo doc` or `cargo rustc` (or `cargo doc --open`).

### `--compiletest`

Super nice for debugging rustdoc tests. I.e., tests found in the [`rust-lang/rust` repo][rust-repo] under `tests/rustdoc{,-ui,-json}/`. You can run rustdoc on such files simply by calling `rrustdoc file.rs -T` (or `rrustdoc file.rs -To`). `rrustdoc` supports all [`ui_test`]-style [`compiletest`] directives that are relevant (it skips and warns on “unknown” directives).

This build mode can be used to debug *cross-crate re-export* tests found in `tests/rustdoc/inline_cross` (since it understands the directives `//@ aux-build`, `//@ aux-crate`, etc.).

## Stability

Presently this tool has no stability guarantees whatsoever. Anything may change in a new version without notice.

The *default* and the *cross crate* build modes is pretty fleshed out and should be pretty stable.
On the other hand, you might be experience some bugs in the *compiletest* build mode since it was added pretty recently and hasn't been thoroughly tested yet.

The *compiletest+query* build mode (`-TQ`) has not been implemented yet. The plan is to provide useful output for quickly debugging tests making use of [`htmldocck`] and [`jsondocck`] directives.

Feel free to report any bugs or other unpleasantries on [the issue tracker][bugs].
If `rrustdoc -T` fails to build a `tests/rustdoc{,-ui,-json}/` file, e.g., due to unsupported directives, that's definitely a bug.

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

Except as otherwise noted, the contents of this repository are licensed under the MIT license (see the license file). Some files include or are accompanied by explicit license notices.

[x-crate-reexport-bugs]: https://github.com/rust-lang/rust/labels/A-cross-crate-reexports
[rust-repo]: https://github.com/rust-lang/rust
[`ui_test`]: https://github.com/oli-obk/ui_test
[`compiletest`]: https://github.com/rust-lang/rust/tree/master/src/tools/compiletest
[`htmldocck`]: https://github.com/rust-lang/rust/blob/master/src/etc/htmldocck.py
[`jsondocck`]: https://github.com/rust-lang/rust/tree/master/src/tools/jsondocck
[bugs]: https://github.com/fmease/rrustdoc/issues
