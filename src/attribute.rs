use crate::{
    data::{CrateName, CrateNameRef, CrateType, Edition},
    diagnostic::{debug, fmt},
    utility::{
        SmallVec,
        parse::{At, SourceFileParser, Span},
    },
};
use ra_ap_rustc_lexer::{DocStyle, LiteralKind, Token, TokenKind};
use std::{io::Write as _, ops::ControlFlow};

#[cfg(test)]
mod test;

// FIXME: Add comment why we need this! Something like:
//
//   Looking at the “dynamic” crate name makes the most sense I think and
//   is probably what the user intended. Alternatively, we could compute
//   the crate name from the file path and use it in `-o lib$NAME.rlib`
//   and `--extern=$NAME=lib$NAME.rlib`.
//   There is no `rustc --print=crate-type`.
//   Very useful so users don't need to manually specify `--crate-type/-y`

#[derive(Default)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) struct Attrs<'src> {
    pub(crate) crate_name: Option<CrateNameRef<'src>>,
    pub(crate) crate_type: Option<CrateType>,
}

impl<'src> Attrs<'src> {
    pub(crate) fn parse(source: &'src str, edition: Edition, verbose: bool) -> Self {
        let attrs = AttrParser::new(source, edition).execute();
        let amount = attrs.len();

        if verbose {
            let s = if amount == 1 { "" } else { "s" };
            debug(fmt!("parser: found {amount} crate attribute{s}")).finish();
        }

        let attrs = Self::lower(attrs, source);

        if amount != 0 && verbose {
            let verb = |present| if present { "found" } else { "did not find" };

            debug(|p| {
                let verb = verb(attrs.crate_name.is_some());
                write!(p, "lowerer: {verb} a (well-formed) `#![crate_name]`")?;
                if let Some(crate_name) = attrs.crate_name {
                    write!(p, ": `{crate_name}`")?;
                }
                Ok(())
            })
            .finish();

            debug(|p| {
                let verb = verb(attrs.crate_type.is_some());
                write!(p, "lowerer: {verb} a (well-formed) `#![crate_type]`",)?;
                if let Some(crate_type) = attrs.crate_type {
                    write!(p, ": `{}`", crate_type.to_str())?;
                }
                Ok(())
            })
            .finish();
        }

        attrs
    }

    fn lower(attrs: Vec<Attr<'src>>, source: &'src str) -> Self {
        let mut crate_name = None;
        let mut crate_type = None;

        for attr in attrs {
            // We found the attributes we're interested in, we can stop processing.
            if crate_name.is_some() && crate_type.is_some() {
                break;
            }

            let Some(ident) = attr.path.ident() else {
                continue;
            };

            let extract_eq_str = || {
                if let Some(Meta::Assignment { value: expr }) = attr.meta
                    && let [(token, span)] = &*expr
                    && let TokenKind::Literal { kind: literal, .. } = token
                {
                    let (start, end) = match literal {
                        LiteralKind::Str { terminated: true } => (1, 1),
                        LiteralKind::RawStr { n_hashes } => {
                            let n_hashes = n_hashes.unwrap_or(0) as usize;
                            (1 + 1 + n_hashes, 1 + n_hashes)
                        }
                        _ => return None,
                    };

                    // FIXME: Unescape escape sequences inside `source` if we have `LiteralKind::Str`.
                    let source = source.at(*span);
                    Some(&source[..source.len().saturating_sub(end)][start..])
                } else {
                    None
                }
            };

            // We don't need to support `crate_{name,type}` inside `cfg_attr` because that's a hard error since 1.83.
            // See also rust-lang/rust#91632.
            match ident {
                "crate_name" => {
                    // We don't need to care about anything other than string literals since everything else
                    // gets rejected semantically by rustc.
                    // `#![crate_name]` used to support macro calls as the expression — by accident, I think.
                    // PR rust-lang/rust#117584 accidentally broke this. Tracked in issue rust-lang/rust#122001.
                    // T-lang has rules to make it a semantic error. Implemented in PR rust-lang/rust#127581
                    // (to be approved and merged).
                    if crate_name.is_none()
                        && let Some(name) = extract_eq_str()
                    {
                        crate_name = Some(match CrateName::parse(name) {
                            Ok(name) => name,
                            Err(()) => break, // like in rustc, an invalid crate name is fatal
                        });
                    }
                }
                "crate_type" => {
                    // We don't need to care about anything other than string literals since everything else
                    // gets rejected semantically by rustc.
                    if crate_type.is_none()
                        && let Some(type_) = extract_eq_str()
                    {
                        // Note this only accepts `lib`, `rlib`, `bin` and `proc-macro` at the time of writing.
                        // FIXME: At least warn on types unsupported by rruxwry.
                        crate_type = Some(match type_.parse() {
                            Ok(type_) => type_,
                            Err(_) => continue, // like in rustc, an invalid crate type is non-fatal
                        });
                    }
                }
                _ => {}
            }
        }

        Self { crate_name, crate_type }
    }
}

struct AttrParser<'src> {
    parser: SourceFileParser<'src>,
    edition: Edition,
}

impl<'src> AttrParser<'src> {
    fn new(source: &'src str, edition: Edition) -> Self {
        Self { parser: SourceFileParser::new(source), edition }
    }

    fn execute(mut self) -> Vec<Attr<'src>> {
        let mut attrs = Vec::new();

        while let () = self.parse_trivia()
            && let Some(token) = self.parser.peek()
        {
            let token = Token { ..*token }; // `Token` doesn't impl `Copy` for no apparent reason.
            match token.kind {
                TokenKind::LineComment { doc_style: Some(DocStyle::Inner) }
                | TokenKind::BlockComment { doc_style: Some(DocStyle::Inner), terminated: true } => {
                    self.parser.advance();
                }
                TokenKind::Pound => {
                    self.parser.advance();
                    match self.finish_parsing_inner_attr() {
                        ControlFlow::Continue(attribute) => {
                            attrs.push(attribute);
                        }
                        ControlFlow::Break(()) => break,
                    }
                }
                // Either the source is syntactically malformed or we found an item, an outer doc comment or the `Eof`.
                // In any case we can stop processing the source.
                _ => break,
            }
        }

        attrs
    }

    /// Finish parsing an inner attribute assuming the leading `#` has already been parsed.
    fn finish_parsing_inner_attr(&mut self) -> ControlFlow<(), Attr<'src>> {
        self.parse_trivia();
        // This `Break`s if this is the start of an outer attribute which is exactly what we want:
        // Once we encounter an outer attribute we know for a fact that no more inner attributes
        // may follow, otherwise that would be a syntax error.
        self.parse(TokenKind::Bang)?;
        self.parse_trivia();
        self.parse(TokenKind::OpenBracket)?;

        let path = self.parse_attr_path()?;

        self.parse_trivia();
        let meta = match self.peek()?.kind {
            TokenKind::CloseBracket => None,
            // FIXME: this also triggers on `==`.
            TokenKind::Eq => {
                self.parser.advance();

                // This is an overapproximation for simplicity.
                let value = self.parse_token_stream_until(Delimiter::Bracket)?;

                Some(Meta::Assignment { value })
            }
            kind @ (TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::OpenBrace) => {
                let delimiter = match kind {
                    TokenKind::OpenParen => Delimiter::Parenthesis,
                    TokenKind::OpenBracket => Delimiter::Bracket,
                    TokenKind::OpenBrace => Delimiter::Brace,
                    _ => unreachable!(),
                };
                self.parser.advance();

                let _tokens = self.parse_token_stream_until(delimiter)?;

                self.parse(match delimiter {
                    Delimiter::Parenthesis => TokenKind::CloseParen,
                    Delimiter::Bracket => TokenKind::CloseBracket,
                    Delimiter::Brace => TokenKind::CloseBrace,
                })?;

                Some(Meta::Parenthesized)
            }
            _ => return ControlFlow::Break(()),
        };

        self.parse_trivia();
        self.parse(TokenKind::CloseBracket)?;

        ControlFlow::Continue(Attr { path, meta })
    }

    fn parse_attr_path(&mut self) -> ControlFlow<(), Path<'src>> {
        self.parse_trivia();

        let is_absolute = self.parse_path_separator().is_continue();

        let mut segments = SmallVec::default();

        self.parse_trivia();
        segments.push(self.parse_path_segment_ident()?);

        while let () = self.parse_trivia()
            && let ControlFlow::Continue(()) = self.parse_path_separator()
        {
            self.parse_trivia();
            segments.push(self.parse_path_segment_ident()?);
        }

        ControlFlow::Continue(Path { is_absolute, segments })
    }

    // FIXME: If this encounters `:T` where `T` stands for any token other than `:`, this advances the iterator
    // by one step while it shouldn't advance at all! Somehow parse in a snapshot here!
    fn parse_path_separator(&mut self) -> ControlFlow<()> {
        // NB: No `parse_trivia` in between the `parse(Colon)` calls!
        self.parse(TokenKind::Colon)?;
        self.parse(TokenKind::Colon)
    }

    fn parse_path_segment_ident(&mut self) -> ControlFlow<(), &'src str> {
        // FIXME: what about `InvalidIdent`?
        let ident = match self.peek()?.kind {
            TokenKind::Ident => {
                let ident = self.parser.source();
                if !is_path_segment_keyword(ident) && is_reserved(ident, self.edition) {
                    return ControlFlow::Break(());
                }
                ident
            }
            TokenKind::RawIdent => {
                // Indeed, raw identifiers are *syntactically* disallowed from being path segment keywords.
                // However, we don't want to bail out with an error because rustc doesn't do so either when
                // `--print=crate-name` is passed. Well, next to printing the crate name it also emits an
                // error and exists with a non-zero exit code. Very weird but whatever.
                &self.parser.source()["r#".len()..]
            }
            _ => return ControlFlow::Break(()),
        };
        self.parser.advance();
        ControlFlow::Continue(ident)
    }

    fn parse_token_stream_until(
        &mut self,
        query: Delimiter,
    ) -> ControlFlow<(), SmallVec<(TokenKind, Span), 1>> {
        self.parse_trivia();

        let mut tokens = SmallVec::new();
        let mut stack = Vec::new();
        let mut is_delimited = false;

        while let () = self.parse_trivia()
            && let Some(token) = self.parser.peek()
        {
            let delimiter = match token.kind {
                TokenKind::OpenParen => Some((Delimiter::Parenthesis, Orientation::Opening)),
                TokenKind::OpenBracket => Some((Delimiter::Bracket, Orientation::Opening)),
                TokenKind::OpenBrace => Some((Delimiter::Brace, Orientation::Opening)),
                TokenKind::CloseParen => Some((Delimiter::Parenthesis, Orientation::Closing)),
                TokenKind::CloseBracket => Some((Delimiter::Bracket, Orientation::Closing)),
                TokenKind::CloseBrace => Some((Delimiter::Brace, Orientation::Closing)),
                _ => None,
            };

            if let Some((delimiter, orientation)) = delimiter {
                if stack.is_empty()
                    && delimiter == query
                    && let Orientation::Closing = orientation
                {
                    is_delimited = true;
                    break;
                }

                match orientation {
                    Orientation::Opening => stack.push(delimiter),
                    Orientation::Closing => {
                        let closing_delimiter = delimiter;
                        match stack.pop() {
                            Some(opening_delimiter) if opening_delimiter == closing_delimiter => {}
                            _ => return ControlFlow::Break(()),
                        }
                    }
                }
            }

            tokens.push((token.kind, self.parser.span()));
            self.parser.advance();
        }

        if is_delimited && stack.is_empty() {
            ControlFlow::Continue(tokens)
        } else {
            ControlFlow::Break(())
        }
    }

    fn parse_trivia(&mut self) {
        while let Some(token) = self.parser.peek() {
            match token.kind {
                TokenKind::Whitespace
                | TokenKind::LineComment { doc_style: None }
                | TokenKind::BlockComment { doc_style: None, terminated: _ } => {
                    self.parser.advance();
                }
                _ => break,
            }
        }
    }

    fn parse(&mut self, predicate: impl Predicate) -> ControlFlow<()> {
        if predicate.execute(self.peek()?.kind) {
            self.parser.advance();
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }

    fn peek(&mut self) -> ControlFlow<(), &Token> {
        match self.parser.peek() {
            Some(token) => ControlFlow::Continue(token),
            None => ControlFlow::Break(()),
        }
    }
}

#[derive(Debug)]
struct Attr<'src> {
    path: Path<'src>,
    meta: Option<Meta>,
}

#[derive(Debug)]
struct Path<'src> {
    /// Whether this path starts with `::`.
    is_absolute: bool,
    segments: SmallVec<&'src str, 1>,
}

impl<'src> Path<'src> {
    fn ident(&self) -> Option<&'src str> {
        if !self.is_absolute
            && let [ident] = *self.segments
        {
            Some(ident)
        } else {
            None
        }
    }
}

#[derive(Debug)]
enum Meta {
    Parenthesized,
    Assignment { value: SmallVec<(TokenKind, Span), 1> },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Delimiter {
    /// Round brackets / parentheses: `(` or `)`.
    Parenthesis,
    /// Square brackets: `[` or `]`.
    Bracket,
    /// Curly brackets / curly braces: `{` or `}`.
    Brace,
}

enum Orientation {
    Opening,
    Closing,
}

fn is_reserved(ident: &str, edition: Edition) -> bool {
    #[rustfmt::skip]
    fn is_used_keyword(ident: &str) -> bool {
        matches!(
            ident,
            | "as" | "break" | "const" | "continue" | "crate" | "else" | "enum" | "extern" | "false" | "fn"
            | "for" | "if" | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut"
            | "pub" | "ref" | "return" | "self" | "Self" | "static" | "struct" | "super" | "trait" | "true"
            | "type" | "unsafe" | "use" | "where" | "while"
        )
    }

    #[rustfmt::skip]
    fn is_unused_keyword(ident: &str) -> bool {
        matches!(
            ident,
            | "abstract" | "become" | "box" | "do" | "final" | "macro" | "override" | "priv" | "typeof" | "unsized"
            | "virtual" | "yield"
        )
    }

    fn is_used_keyword_if(ident: &str, edition: Edition) -> bool {
        edition >= Edition::Rust2018 && matches!(ident, "async" | "await" | "dyn")
    }

    fn is_unused_keyword_if(ident: &str, edition: Edition) -> bool {
        edition >= Edition::Rust2018 && matches!(ident, "try")
            || edition >= Edition::Rust2024 && matches!(ident, "gen")
    }

    ident == "_"
        || is_used_keyword(ident)
        || is_unused_keyword(ident)
        || is_used_keyword_if(ident, edition)
        || is_unused_keyword_if(ident, edition)
}

fn is_path_segment_keyword(ident: &str) -> bool {
    matches!(ident, "_" | "self" | "Self" | "super" | "crate")
}

trait Predicate {
    fn execute(self, query: TokenKind) -> bool;
}

impl<F: FnOnce(TokenKind) -> bool> Predicate for F {
    fn execute(self, query: TokenKind) -> bool {
        (self)(query)
    }
}

impl Predicate for TokenKind {
    fn execute(self, query: TokenKind) -> bool {
        self == query
    }
}
