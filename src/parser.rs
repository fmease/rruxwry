use crate::utility::Captures;
use ra_ap_rustc_lexer::{strip_shebang, tokenize, Token};
use std::iter::Peekable;

pub(crate) struct SourceFileParser<'src> {
    tokens: Peekable<Tokens<'src>>,
    source: &'src str,
    index: usize,
}

impl<'src> SourceFileParser<'src> {
    pub(crate) fn new(source: &'src str) -> Self {
        let index = strip_shebang(source).unwrap_or_default();
        let tokens = tokenize(&source[index..]).peekable();

        Self {
            tokens,
            source,
            index,
        }
    }

    pub(crate) fn peek(&mut self) -> Option<&Token> {
        self.tokens.peek()
    }

    pub(crate) fn source(&mut self) -> &'src str {
        self.source.at(self.span())
    }

    pub(crate) fn span(&mut self) -> Span {
        Span {
            start: self.index as _,
            length: self.peek().map_or(0, |token| token.len),
        }
    }

    pub(crate) fn advance(&mut self) {
        if let Some(token) = self.peek() {
            self.index += token.len as usize;
            self.tokens.next();
        }
    }
}

type Tokens<'src> = impl Iterator<Item = Token> + Captures<'src>;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Span {
    start: u32,
    length: u32,
}

pub(crate) trait At<'src> {
    type Output;

    fn at(self, span: Span) -> Self::Output;
}

impl<'src> At<'src> for &'src str {
    type Output = &'src str;

    fn at(self, span: Span) -> Self::Output {
        &self[span.start as _..][..span.length as _]
    }
}
