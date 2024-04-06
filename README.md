# rrustdoc – A rustdoc wrapper for rustdoc devs

A power tool for rustdoc devs that wraps `rustdoc` and `rustc`.

Its most useful features are the flags `-o`/`--open`, `-X`/`--cross-crate` and `-T`/`--compiletest`.

## Power Features

### `--cross-crate`

Very useful for rapidly reproducing or debugging [rustdoc *cross-crate re-exports* issues][x-crate-reexport-bugs].
*rrustdoc* reduces the number of steps from many to just two:

1. Creating a single file `file.rs`,
2. Executing `rrustdoc file.rs -X` (or `rrustdoc file.rs -Xo`).

The alternatives would be:

* Creating at least two files, running `rustc` then `rustdoc` manually without forgetting the various flags that need to be passed and manually opening up the generated documentation with the browser of choice.
* Setting up a *[Cargo]* project containing at least two crates and running `cargo doc` or `cargo rustc` (or `cargo doc --open`).

### `--compiletest`

Super nice for debugging rustdoc tests. I.e., tests found in the [rust-lang/rust repository][rust-repo] under `tests/rustdoc{,-ui,-json}/`. You can run rustdoc on such files simply by calling `rrustdoc file.rs -T` (or `rrustdoc file.rs -To`). *rrustdoc* supports all [`ui_test`]-style [`compiletest`] directives that are relevant (it skips and warns on “unknown” directives).

This build mode can be used to debug *cross-crate re-export* tests found in `tests/rustdoc/inline_cross` (since it understands the directives `//@ aux-build`, `//@ aux-crate`, etc.).

## Stability

Presently this tool has no stability guarantees whatsoever. Anything may change in a new version without notice.

The *default* and the *cross crate* build modes is pretty fleshed out and should be pretty stable.
On the other hand, you might experience some bugs in the *compiletest* build mode since it was added pretty recently and hasn't been thoroughly tested yet.

The *compiletest+query* build mode (`-TQ`) has not been implemented yet. The plan is to provide useful output for quickly debugging tests that make use of [`htmldocck`] and [`jsondocck`] directives.

Feel free to report any bugs or other unpleasantries on [the issue tracker][bugs].
If `rrustdoc -T` fails to build a `tests/rustdoc{,-ui,-json}/` file, e.g., due to unsupported directives, that's definitely a bug.

## Explainer & Tutorial

### Default Build Mode

Compared to a raw `rustdoc file.rs` invocation, a `rrustdoc file.rs` invocation has some goodies in store.

For one, it defaults to the *latest stable edition* (i.e., Rust 2021 at the time of writing) while `rustdoc` obviously defaults to the first edition (i.e., Rust 2015) for backward compatibility. You can pass `-e`/`--edition` to overwrite the edition.

Moreover, you don't need to “pass `-Zunstable-options`” (that flag is not even available) since *rrustdoc* does that for you (this is a *developer tool* after all).

If you are documenting a proc-macro crate, *rrustdoc* automatically adds `proc_macro` to the *extern prelude* similar to *Cargo* so you should never need to write `extern crate proc_macro;` in Rust ≥2018 crates.

Lastly, `rrustdoc` understands `#![crate_name]` and `#![crate_type]`. One might think that that's a given but there's a significant amount of work involved to support these attributes. *Cargo* for example doesn't understand them requiring you to categorize your crates in `Cargo.toml` via the sections `[lib]`, `[[bin]]` etc. (obviously that's not the actual reason; it's an intentional design decision).

### Cross-Crate Build Mode

This mode builds upon the default build mode and inherits its basic behavior.

The way *rustdoc* documents user-written code in the local / root crate significantly differs from the way it documents *(inlined) cross-crate re-exports*. In the former case, it processes [HIR] data types, in the latter it processes `rustc_middle::ty` data types. Since `rustc_middle::ty` data types are even more removed from source code than the HIR, there's a lot of work involved inside *rustdoc* to “reconstruct” or “resugar” them to something that looks closer to source code (note that it's close to impossible to perfectly / losslessly reconstruct the `rustc_middle::ty` to HIR-like data types). This has been and still is the source of a lot of *rustdoc* bugs.

We can easily trigger this code path by creating a dependent crate containing `pub use krate::*;` re-exporting the crate `krate` we're actually interested in. `rrustdoc -X` does this step for us. It generates a dummy crate called `u_⟨name⟩` and invokes `rustc` & `rustdoc` for us.

In summary, you don't need to do anything except passing `-X`, your file can remain unchanged.

NB: If you have previously run the default build mode and passed `-o` to open the generated documentation, you need to pass `-o` “again” when you'd like run the cross-crate build mode and open the generated docs since you want to see the docs for crate `u_⟨name⟩`, *not* `⟨name⟩`. Just something to be aware of.

`--private` and `--hidden` aren't meaningful in cross-crate mode (**FIXME**: Would they meaningful if we did the same as `//@ build-aux-docs`, e.g. if the user passes `-XX`? Otherwise just reject those flags).

### Compiletest Build Mode

This mode is entirely separate from the default & the cross-crate build mode.

> **FIXME**: Expand upon this section.

*rrustdoc* natively understands the following [`ui_test`]-style [`compiletest`] directives: `aux-build`, `aux-crate`, `build-aux-docs`, `compile-flags`, `edition`, `force-host` (**FIXME**: Well, we ignore it right now), `no-prefer-dynamic` (**FIXME**: Well, we ignore it right now), `revisions`, `rustc-env` and `unset-rustc-env`. Any other directives get skipped and *rrustdoc* emits a warning for the sake of transparency. This selection should suffice, it should cover the majority of use cases. We intentionally don't support `{unset-,}exec-env` since it's not meaningful.

*rrustdoc* has *full* support for *revisions*. You can pass `--cfg` to enable individual revisions. In the future, *rrustdoc* will have support for `--rev` (the same as `--cfg` except that we check that the given revision was actually declared with `//@ revisions`) and `--all-revs` (executing `rrustdoc` (incl. `--open`) for all declared revisions; useful for swiftly comparing minor changes to the source code).

### Features Common Across Build Modes

For convenience, you can pass `-f`/`--cargo-feature` `⟨NAME⟩` to enable a *Cargo*-like feature, i.e., a `cfg` that can be checked for with `#[cfg(feature = "⟨NAME⟩")]` and similar in the source code. `-f ⟨NAME⟩` just expands to `--cfg feature="⟨NAME⟩"` (modulo shell escaping).

For convenience, you can pass `-F`/`--rustc-feature` `⟨NAME⟩` to enable an experimental rustc library or language feature. It just expands to `-Zcrate-attr=feature(⟨NAME⟩)` (modulo shell escaping). For example, you can add `-Flazy_type_alias` to quickly enable *[lazy type aliases]*.

To set the *[rustup]* toolchain, you use `-t`. Examples: `rrustdoc file.rs -tnightly`, `rrustdoc file.rs -tstage2`. Currently, you *cannot* use the *rustup*-style `+⟨TOOLCHAIN⟩` flag unfortunately. I plan on adding support for that if there's an easy way to do it with `clap` (the CLI parser we use).

## Command-Line Interface

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
[Cargo]: https://github.com/rust-lang/cargo/
[rust-repo]: https://github.com/rust-lang/rust
[`ui_test`]: https://github.com/oli-obk/ui_test
[`compiletest`]: https://github.com/rust-lang/rust/tree/master/src/tools/compiletest
[`htmldocck`]: https://github.com/rust-lang/rust/blob/master/src/etc/htmldocck.py
[`jsondocck`]: https://github.com/rust-lang/rust/tree/master/src/tools/jsondocck
[bugs]: https://github.com/fmease/rrustdoc/issues
[HIR]: https://rustc-dev-guide.rust-lang.org/hir.html#the-hir
[lazy type aliases]: https://github.com/rust-lang/rust/issues/112792
[rustup]: https://github.com/rust-lang/rustup/
