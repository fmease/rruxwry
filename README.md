<h1 align="center">——— rruxwry ———</h1>

<p align="center">A power tool for rustc & rustdoc devs that wraps <code>rustc</code> and <code>rustdoc</code>.</p>

> [!IMPORTANT]
> This project is undergoing major reworks and is not yet suitable for general use.

<!--

FIXME: Rework the whole commented-out section. It's completely out of date!

## Introduction

> [!NOTE]
> This section is incomplete.

It's subsequently assumed that `rruxwry build` and `rruxwry doc` were shell-aliased to `rrc` and `rrd`, respectively.

### `-@`, `--compiletest`

Super nice for debugging rustdoc tests. I.e., tests found in the [rust-lang/rust repository][rust-repo] under `tests/rustdoc{,-ui,-json}/`. You can run rustdoc on such files simply by calling `rrd file.rs -@` (or `rrd file.rs -@o`). *rruxwry* supports all [`ui_test`]-style [`compiletest`] directives that are relevant (it skips and warns on “unknown” directives).

This build mode can be used to debug *cross-crate re-export* tests found in `tests/rustdoc/inline_cross` (since it understands the directives `//@ aux-build`, `//@ aux-crate`, etc.).

### `-X`, `--cross-crate`

Very useful for rapidly reproducing and debugging [rustdoc *cross-crate re-exports* issues][x-crate-reexport-bugs].
*rruxwry* reduces the number of steps from many to just two:

1. Creating a single file `file.rs`,
2. Executing `rrd file.rs -X` (or `rrd file.rs -Xo`).

The alternatives would be:

* Creating at least two files, running `rustc` then `rustdoc` manually without forgetting the various flags that need to be passed and manually opening up the generated documentation with the browser of choice.
* Setting up a *[Cargo]* project containing at least two crates and running `cargo doc` or `cargo rustc` (or `cargo doc --open`).

## Stability

**Presently this tool has no stability guarantees whatsoever**. Anything may change in a new version without notice.

The *default* and the *cross crate* build modes are pretty fleshed out and should be pretty stable.
On the other hand, you might experience some bugs in the *compiletest* build mode since it was added pretty recently and hasn't been thoroughly tested yet.

Feel free to report any bugs and other unpleasantries on [the issue tracker][bugs].
If `rruxwry {b,d} -@` fails to build a `tests/rustdoc{,-ui,-json}/` file, e.g., due to unsupported directives, that's definitely a bug. [!-- FIXME: Change policy --]

## Explainer & Tutorial

> [!WARNING]
> This section is severely out of date.

### Default Build Mode

Compared to a raw `rustdoc file.rs` invocation, a `rrd file.rs` invocation has some goodies in store.

For one, it defaults to the *latest stable edition* (i.e., Rust 2024 at the time of writing) while `rustdoc` obviously defaults to the first edition (i.e., Rust 2015) for backward compatibility. You can pass `-e`/`--edition` to overwrite the edition.

Moreover, you don't need to “pass `-Zunstable-options`” (that flag is not even available) since *rruxwry* does that for you (this is a *developer tool* after all).

If you are documenting a proc-macro crate, *rruxwry* automatically adds `proc_macro` to the *extern prelude* similar to *Cargo* so you should never need to write `extern crate proc_macro;` in Rust ≥2018 crates.

*rruxwry* understands `#![crate_name]` and `#![crate_type]`. One might think that that's a given but there's a significant amount of work involved to support these attributes. *Cargo* for example doesn't understand them requiring you to categorize your crates in `Cargo.toml` via the sections `[lib]`, `[[bin]]` etc. (obviously that's not the actual reason; it's an intentional design decision). In the unlikely case of *rruxwry* not recognizing the crate name or crate type from the crate attributes, you can set them explicitly with `-n`/`--crate-name` and `-t`/`--crate-type` respectively.

Lastly, *rruxwry* defaults to the CSS theme *Ayu* because it's a dark theme and it looks very nice :P

### Cross-Crate Build Mode

This mode builds upon the default build mode and inherits its basic behavior.

The way *rustdoc* documents user-written code in the local / root crate significantly differs from the way it documents *(inlined) cross-crate re-exports*. In the former case, it processes [HIR] data types, in the latter it processes `rustc_middle::ty` data types. Since `rustc_middle::ty` data types are even more removed from source code than the HIR, there's a lot of work involved inside *rustdoc* to “reconstruct” or “resugar” them to something that looks closer to source code (note that it's close to impossible to perfectly / losslessly reconstruct the `rustc_middle::ty` to HIR-like data types). This has been and still is the source of a lot of *rustdoc* bugs.

We can easily trigger this code path by creating a dependent crate containing `pub use krate::*;` re-exporting the crate `krate` we're actually interested in. `rrd -X` does this step for us. It generates a dummy crate called `u_⟨name⟩` and invokes `rustc` & `rustdoc` for us.

In summary, you don't need to do anything except passing `-X`, your file can remain unchanged.

NB: If you have previously run the default build mode and passed `-o` to open the generated documentation, you need to pass `-o` “again” when you'd like run the cross-crate build mode and open the generated docs since you want to see the docs for crate `u_⟨name⟩`, *not* `⟨name⟩`. Just something to be aware of.

`--private` and `--hidden` aren't meaningful in cross-crate mode (**FIXME**: Would they be meaningful if we did the same as `//@ build-aux-docs`, e.g. if the user passes `-XX`? Otherwise just reject those flags).

### Compiletest Build Mode

This mode is entirely separate from the default & the cross-crate build mode.

[!-- FIXME: Expand upon this section. --]

*rruxwry* natively understands the following [`ui_test`]-style [`compiletest`] directives: `aux-build`, `aux-crate`, `build-aux-docs`, `compile-flags`, `edition`, `force-host`[!-- FIXME: Well, we ignore it right now --], `no-prefer-dynamic`[!-- FIXME: Well, we ignore it right now --], `revisions`, `rustc-env` and `unset-rustc-env`. Any other directives get skipped and *rruxwry* emits a warning for the sake of transparency. This selection should suffice, it should cover the majority of use cases. We intentionally don't support `{,unset-}exec-env` since it's not meaningful.

*rruxwry* has *full* support for *revisions*. You can pass `--rev ⟨NAME⟩` or `--cfg ⟨SPEC⟩` to enable individual revisions. The former is checked against the revisions declared by `//@ revisions`, the latter is *not*. In the future, *rruxwry* will have support for `--all-revs` (executing *rruxwry* (incl. `--open`) for all declared revisions; useful for swiftly comparing minor changes to the source code).

### Features Common Across Build Modes

If the argument right after the subcommand (i.e., the second argument) begins with a `+`, it will be interpreted as a *[rustup]* toolchain name. Examples: `rruxwry {b,d} +nightly file.rs`, `rruxwry {b,d} +stage2 file.rs`.

You can pass the convenience flag `-f`/`--cargo-feature` `⟨NAME⟩` to enable a *Cargo*-like feature, i.e., a `cfg` that can be checked for with `#[cfg(feature = "⟨NAME⟩")]` and similar in the source code. `-f ⟨NAME⟩` just expands to `--cfg feature="⟨NAME⟩"` (modulo shell escaping).

You can pass the convenience flag `-F`/`--rustc-feature` `⟨NAME⟩` to enable an experimental rustc library or language feature. It just expands to `rust{,do}c`'s `-Zcrate-attr=feature(⟨NAME⟩)` (modulo shell escaping). For example, you can pass `-Flazy_type_alias` to quickly enable *[lazy type aliases]*.

If you'd like to know the precise commands *rruxwry* runs under the hood for example to be able to open a rust-lang/rust GitHub issue with proper reproduction steps, pass `-V`/`--verbose` and look for output of the form `info: running `. *rruxwry* tries very hard to minimize the amount of flags passed to `rust{,do}c` exactly for the aforementioned use case. It's not perfect, you might be able to remove some flags for the reproducer (you can definitely get rid of `--default-theme=ayu` :D).

Just like *Cargo*, *rruxwry* recognizes the environment variables `RUSTFLAGS` and `RUSTDOCFLAGS`. The arguments / flags present in these flags get passed *verbatim* (modulo shell escaping) to `rustc` and `rustdoc` respectively. Be aware that the flags you pass *may conflict* with the ones added by *rruxwry* but as mentioned in the paragraph above, it tries fiercely to not add flags unnecessarily. Note that your flags get added last. You can debug conflicts by passing `-V`/`--verbose` to `rruxwry` and by looking for lines starting with `info: running ` in the output to get to know first hand what `rruxwry` tried to pass to the underlying programs.

However if that's too wordy for you and you don't care about passing arguments / flags to *both* `rustc` *and* `rustdoc`, you can simply provide them inline after `--`. Example: `rrd file.rs -X -- -Ztreat-err-as-bug`. Here, the `-Z` flag gets passed to both `rustc file.rs` and `rustdoc u_file.rs` (remember, `-X` enables the cross-crate build mode).

`-e`/`--edition` supports the following edition *aliases*: `D` (rustc's **d**efault edition), `S` (latest **s**table edition) and `E` ("bleeding **e**dge": latest edition, no matter if stable or unstable).

-->

## Command-Line Interface

<!--{COMMAND-->
`rruxwry -h`:
```
A wrapper around rust{,do}c for rust{,do}c devs

Usage: rruxwry <COMMAND>

Commands:
  build  Compile the given crate with rustc
  doc    Document the given crate with rustdoc
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```
<!--COMMAND}-->

<!--{COMMAND-->
`rruxwry build -h`:
```
Compile the given crate with rustc

Usage: rruxwry build [OPTIONS] [PATH] [-- [VERBATIM]...]

Arguments:
  [PATH]         Path to the source file
  [VERBATIM]...  Flags passed to `rustc` verbatim

Options:
  -r, --run                    Also run the built binary
  -c, --check-only             Don't fully compile, only check the crate
  -@, --directives[=<FLAVOR>]  Enable compiletest-like directives
  -T, --compiletest            Check in a compiletest-esque manner
  -., --bless                  Update the test expectations
  -n, --crate-name <NAME>      Set the name of the crate
  -t, --crate-type <TYPE>      Set the type of the crate
  -e, --edition <EDITION>      Set the edition of the crate
      --cfg <NAME[="VALUE"]>   Enable a configuration
  -R, --revision <NAME>        Enable a compiletest revision
  -F, --feature <NAME>         Enable an experimental library or language feature
  -s, --shallow                Halt after parsing the source file
  -x, --extern <NAME>          Register an external library
  -/, --suppress-lints         Cap lints at allow level
  -#, --internals              Enable internal pretty-printing of data types
  -N, --next-solver            Enable the next-gen trait solver
  -I, --identity <IDENTITY>    Force rust{,do}c's identity
  -D, --no-dedupe              Don't deduplicate diagnostics
      --log[=<FILTER>]         Enable rust{,do}c logging. FILTER defaults to `debug`
  -B, --no-backtrace           Override `RUST_BACKTRACE` to be `0`
  -V, --version                Print the underlying rust{,do}c version and halt
  -v, --verbose                Use verbose output
      --color <WHEN>           Control when to use color [default: auto] [possible values: auto, always, never]
  -h, --help                   Print help
```
<!--COMMAND}-->

<!--{COMMAND-->
`rruxwry doc -h`:
```
Document the given crate with rustdoc

Usage: rruxwry doc [OPTIONS] [PATH] [-- [VERBATIM]...]

Arguments:
  [PATH]         Path to the source file
  [VERBATIM]...  Flags passed to `rustc` and `rustdoc` verbatim

Options:
  -o, --open                     Also open the generated docs in a browser
  -j, --json                     Output JSON instead of HTML
  -@, --directives[=<FLAVOR>]    Enable compiletest-like directives
  -T, --compiletest              Check in a compiletest-esque manner
  -., --bless                    Update the test expectations
  -X, --cross-crate              Enable the cross-crate re-export mode
  -n, --crate-name <NAME>        Set the name of the crate
  -t, --crate-type <TYPE>        Set the type of the crate
      --crate-version <VERSION>  Set the version of the (base) crate
  -e, --edition <EDITION>        Set the edition of the crate
      --cfg <NAME[="VALUE"]>     Enable a configuration
  -R, --revision <NAME>          Enable a compiletest revision
  -F, --feature <NAME>           Enable an experimental library or language feature
  -P, --private                  Document private items
  -H, --hidden                   Document hidden items
      --layout                   Document the memory layout of types
      --link-to-def              Generate links to definitions
      --normalize                Normalize types
      --theme <THEME>            Set the theme [default: ayu]
  -x, --extern <NAME>            Register an external library
  -/, --suppress-lints           Cap lints at allow level
  -#, --internals                Enable internal pretty-printing of data types
  -N, --next-solver              Enable the next-gen trait solver
  -I, --identity <IDENTITY>      Force rust{,do}c's identity
  -D, --no-dedupe                Don't deduplicate diagnostics
      --log[=<FILTER>]           Enable rust{,do}c logging. FILTER defaults to `debug`
  -B, --no-backtrace             Override `RUST_BACKTRACE` to be `0`
  -V, --version                  Print the underlying rust{,do}c version and halt
  -v, --verbose                  Use verbose output
      --color <WHEN>             Control when to use color [default: auto] [possible values: auto, always, never]
  -h, --help                     Print help
```
<!--COMMAND}-->

Additionally, *rruxwry* recognizes the environment variables `RUSTFLAGS` and `RUSTDOCFLAGS`.

## Name: Pronunciation and Origin

IPA transcription: /ʔə.ˈɹʌks.ɹaɪ/.
Standard phonetic transcription: \[*uh*-**ruhks**-rahy\].

Origin: **r**ustc **r**ustdoc r**u**st{,do}c e**x**ecute **w**rite **r**ead -**y**.

## License

Except as otherwise noted, the contents of this repository are licensed under the MIT license (see the license file). Some files include or are accompanied by explicit license notices.

[x-crate-reexport-bugs]: https://github.com/rust-lang/rust/labels/A-cross-crate-reexports
[Cargo]: https://github.com/rust-lang/cargo/
[rust-repo]: https://github.com/rust-lang/rust
[`ui_test`]: https://github.com/oli-obk/ui_test
[`compiletest`]: https://github.com/rust-lang/rust/tree/master/src/tools/compiletest
[`htmldocck`]: https://github.com/rust-lang/rust/blob/master/src/etc/htmldocck.py
[`jsondocck`]: https://github.com/rust-lang/rust/tree/master/src/tools/jsondocck
[bugs]: https://github.com/fmease/rruxwry/issues
[HIR]: https://rustc-dev-guide.rust-lang.org/hir.html#the-hir
[lazy type aliases]: https://github.com/rust-lang/rust/issues/112792
[rustup]: https://github.com/rust-lang/rustup/
