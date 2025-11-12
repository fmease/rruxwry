<h1 align="center">——— rruxwry ———</h1>

<p align="center">A power tool for rustc & rustdoc devs that wraps <code>rustc</code> and <code>rustdoc</code>.</p>

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
  -:, --source <SOURCE>        Provide the source code
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
  -d, --dump <IR>              Print the given compiler IR
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
  -:, --source <SOURCE>          Provide the source code
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

## Documentation

Presently, there is no further documentation. Good luck!

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
