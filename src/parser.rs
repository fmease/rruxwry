pub(crate) struct SourceFileParser<'src> {
    tokens: lexer::PeekableTokens<'src>,
    source: &'src str,
    index: usize,
}

impl<'src> SourceFileParser<'src> {
    pub(crate) fn new(source: &'src str) -> Self {
        let (index, tokens) = lexer::lex(source);

        Self { tokens, source, index }
    }

    pub(crate) fn peek(&mut self) -> Option<&lexer::Token> {
        self.tokens.peek()
    }

    pub(crate) fn source(&mut self) -> &'src str {
        self.source.at(self.span())
    }

    pub(crate) fn span(&mut self) -> Span {
        Span {
            start: self.index.try_into().unwrap(),
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

mod lexer {
    pub(super) use ra_ap_rustc_lexer::Token;
    use ra_ap_rustc_lexer::strip_shebang;
    use std::iter::Peekable;

    pub(super) type PeekableTokens<'src> = Peekable<Tokens<'src>>;

    pub(super) type Tokens<'src> = impl Iterator<Item = Token>;

    pub(super) fn lex(source: &str) -> (usize, PeekableTokens<'_>) {
        let index = strip_shebang(source).unwrap_or_default();
        let tokens = ra_ap_rustc_lexer::tokenize(&source[index..]).peekable();
        (index, tokens)
    }
}

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
