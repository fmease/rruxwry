use crate::{
    context::Context,
    diagnostic::{error, fmt},
    utility::monotonic::MonotonicVec,
};
use std::{
    fmt,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(crate) struct SourceMap {
    files: MonotonicVec<SourceFileBuf>,
}

impl SourceMap {
    fn offset(&self) -> u32 {
        const PADDING: u32 = 1;
        self.files.last().map(|file| file.span.end).unwrap_or_default() + PADDING
    }

    // FIXME: Detect circular/cyclic imports here.
    pub(crate) fn add(
        &self,
        path: Spanned<SourcePath<'_>>,
        cx: Context<'_>,
    ) -> crate::error::Result<SourceFile<'_>> {
        // We don't care about canonicalization (this is just for caching).
        // We don't care about TOC-TOU (not safety critical).
        if let Some(file) = self.files.iter().find(|file| file.path.as_ref() == path.bare) {
            // FIXME: Safety comment.
            return Ok(unsafe { file.as_ref() });
        }

        let contents = match path.bare {
            SourcePath::Regular(path_) => std::fs::read_to_string(path_).map_err(|error| {
                self::error(fmt!("failed to read `{}`", path_.display()))
                    .highlight(path.span, cx)
                    .note(fmt!("{error}"))
                    .done()
            })?,
            SourcePath::Stdin => std::io::read_to_string(std::io::stdin())?,
        };

        let file = SourceFileBuf::new(path.bare.to_owned(), contents, self.offset());
        // FIXME: Safety comment.
        let result = unsafe { file.as_ref() };
        self.files.push(file);
        Ok(result)
    }

    // FIXME: Use this comment again:
    // UNSAFETY: `project` must return an address to `R` that remains valid even if `T` is moved.

    pub(crate) fn by_span(&self, span: Span) -> Option<SourceFile<'_>> {
        if span.is_sham() {
            return None;
        }
        // FIXME: Perform a binary search by span instead.
        let file = self.files.iter().find(|file| file.span.contains(span.start)).unwrap();
        // FIXME: Safety comment.
        Some(unsafe { file.as_ref() })
    }

    pub(crate) fn get<'a>(&'a self, path: SourcePath<'_>) -> Option<SourceFile<'a>> {
        self.files
            .iter()
            .find(|file| file.path.as_ref() == path)
            // FIXME: Safety comment.
            .map(|file| unsafe { file.as_ref() })
    }
}

struct SourceFileBuf {
    path: SourcePathBuf,
    contents: String,
    span: Span,
}

impl SourceFileBuf {
    fn new(path: SourcePathBuf, contents: String, offset: u32) -> Self {
        let span = Span::new(offset, offset + u32::try_from(contents.len()).unwrap());
        Self { path, contents, span }
    }

    // FIXME: Safety conditions.
    unsafe fn as_ref<'a>(&self) -> SourceFile<'a> {
        // FIXME: Safety comments
        SourceFile {
            path: match self.path {
                SourcePathBuf::Regular(ref path) => {
                    SourcePath::Regular(unsafe { &*std::ptr::from_ref(path.as_path()) })
                }
                SourcePathBuf::Stdin => SourcePath::Stdin,
            },
            contents: unsafe { &*std::ptr::from_ref(self.contents.as_str()) },
            span: self.span,
        }
    }
}

pub(crate) enum SourcePathBuf {
    Regular(PathBuf),
    Stdin,
}

impl SourcePathBuf {
    pub(crate) fn as_ref(&self) -> SourcePath<'_> {
        match self {
            Self::Regular(path) => SourcePath::Regular(&path),
            Self::Stdin => SourcePath::Stdin,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SourceFile<'a> {
    pub(crate) path: SourcePath<'a>,
    pub(crate) contents: &'a str,
    pub(crate) span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourcePath<'a> {
    Regular(&'a Path),
    Stdin,
}

impl SourcePath<'_> {
    fn to_owned(self) -> SourcePathBuf {
        match self {
            Self::Regular(path) => SourcePathBuf::Regular(path.to_owned()),
            Self::Stdin => SourcePathBuf::Stdin,
        }
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub(crate) struct Span<const L: Locality = { Locality::Global }> {
    pub(crate) start: u32,
    pub(crate) end: u32,
}

impl<const L: Locality> Span<L> {
    pub(crate) const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub(crate) const fn empty(index: u32) -> Self {
        Self::new(index, index)
    }

    pub(crate) fn with_len(start: u32, length: u32) -> Self {
        Self::new(start, start + length)
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.start == self.end
    }

    const fn contains(self, index: u32) -> bool {
        self.start <= index && index <= self.end
    }

    const fn unshift(self, offset: u32) -> Self {
        Self::new(self.start - offset, self.end - offset)
    }

    pub(crate) const fn shift(self, offset: u32) -> Self {
        Self::new(self.start + offset, self.end + offset)
    }

    pub(crate) const fn reinterpret<const P: Locality>(self) -> Span<P> {
        Span::new(self.start, self.end)
    }
}

impl Span {
    pub(crate) const SHAM: Self = Self::new(0, 0);

    pub(crate) const fn local(self, file: SourceFile<'_>) -> LocalSpan {
        self.unshift(file.span.start).reinterpret()
    }

    pub(crate) const fn is_sham(self) -> bool {
        self.start == 0 && self.end == 0
    }
}

#[cfg(test)]
impl<const L: Locality> fmt::Debug for Span<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

pub(crate) type LocalSpan = Span<{ Locality::Local }>;

impl LocalSpan {
    #[allow(dead_code)] // FIXME: actually use it
    pub(crate) fn global(self, file: SourceFile<'_>) -> Span {
        self.shift(file.span.start).reinterpret()
    }

    pub(crate) fn range(self) -> std::ops::Range<usize> {
        self.start as usize..self.end as usize
    }
}

#[derive(PartialEq, Eq, std::marker::ConstParamTy)]
pub(crate) enum Locality {
    Local,
    Global,
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub(crate) struct Spanned<T, const L: Locality = { Locality::Global }> {
    pub(crate) span: Span<L>,
    pub(crate) bare: T,
}

impl<T> Spanned<T> {
    pub(crate) fn new(span: Span, bare: T) -> Self {
        Self { span, bare }
    }

    pub(crate) fn sham(bare: T) -> Self {
        Self::new(Span::SHAM, bare)
    }

    pub(crate) fn map<U>(self, mapper: impl FnOnce(T) -> U) -> Spanned<U> {
        Spanned::new(self.span, mapper(self.bare))
    }

    pub(crate) fn as_deref(&self) -> Spanned<&T::Target>
    where
        T: std::ops::Deref,
    {
        Spanned::new(self.span, &self.bare)
    }
}

#[cfg(test)]
impl<T: fmt::Debug> fmt::Debug for Spanned<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{:?} {:?}", self.span, self.bare)
    }
}

impl<T: fmt::Display> fmt::Display for Spanned<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.bare.fmt(f)
    }
}
