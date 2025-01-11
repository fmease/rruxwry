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
    files: MonotonicVec<SourceFile>,
}

impl SourceMap {
    fn offset(&self) -> u32 {
        const PADDING: u32 = 1;
        self.files.last().map(|file| file.span.end).unwrap_or_default() + PADDING
    }

    // FIXME: Detect circular/cyclic imports here.
    pub(crate) fn add(
        &self,
        path: Spanned<&Path>,
        cx: Context<'_>,
    ) -> crate::error::Result<SourceFileRef<'_>> {
        // We don't care about canonicalization (this is just for caching).
        // We don't care about TOC-TOU (not safety critical).
        if let Some(file) = self.files.iter().find(|file| file.path == path.bare) {
            // FIXME: Safety comment.
            return Ok(unsafe { file.as_ref() });
        }

        // FIXME: On `--force` (hypoth), we could suppress the error and
        //        create a sham/dummy SourceFile.
        let contents = std::fs::read_to_string(path.bare).map_err(|error| {
            self::error(fmt!("failed to read `{}`", path.bare.display()))
                .highlight(path.span, cx)
                .note(fmt!("{error}"))
                .finish()
        })?;

        let file = SourceFile::new(path.bare.to_owned(), contents, self.offset());
        // FIXME: Safety comment.
        let result = unsafe { file.as_ref() };
        self.files.push(file);
        Ok(result)
    }

    // FIXME: Use this comment again:
    // UNSAFETY: `project` must return an address to `R` that remains valid even if `T` is moved.

    pub(crate) fn by_span(&self, span: Span) -> Option<SourceFileRef<'_>> {
        if span.is_sham() {
            return None;
        }
        // FIXME: Perform a binary search by span instead.
        let file = self.files.iter().find(|file| file.span.contains(span.start)).unwrap();
        // FIXME: Safety comment.
        Some(unsafe { file.as_ref() })
    }
}

struct SourceFile {
    path: PathBuf,
    contents: String,
    span: Span,
}

impl SourceFile {
    fn new(path: PathBuf, contents: String, offset: u32) -> Self {
        let span = Span::new(offset, offset + u32::try_from(contents.len()).unwrap());
        Self { path, contents, span }
    }

    // FIXME: Safety conditions.
    unsafe fn as_ref<'a>(&self) -> SourceFileRef<'a> {
        // FIXME: Safety comments
        SourceFileRef {
            path: unsafe { &*std::ptr::from_ref(self.path.as_path()) },
            contents: unsafe { &*std::ptr::from_ref(self.contents.as_str()) },
            span: self.span,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SourceFileRef<'a> {
    pub(crate) path: &'a Path,
    pub(crate) contents: &'a str,
    pub(crate) span: Span<{ Locality::Global }>,
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub(crate) struct Span<const L: Locality = { Locality::Global }> {
    pub(crate) start: u32,
    pub(crate) end: u32,
}

impl<const L: Locality> Span<L> {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub(crate) fn with_len(start: u32, length: u32) -> Self {
        Self::new(start, start + length)
    }

    fn contains(self, index: u32) -> bool {
        self.start <= index && index < self.end
    }

    fn unshift(self, offset: u32) -> Self {
        Self::new(self.start - offset, self.end - offset)
    }

    pub(crate) fn shift(self, offset: u32) -> Self {
        Self::new(self.start + offset, self.end + offset)
    }

    pub(crate) fn reinterpret<const P: Locality>(self) -> Span<P> {
        Span::new(self.start, self.end)
    }
}

impl Span {
    pub(crate) const SHAM: Self = Self::new(0, 0);

    pub(crate) fn local(self, file: SourceFileRef<'_>) -> LocalSpan {
        self.unshift(file.span.start).reinterpret()
    }

    pub(crate) fn is_sham(self) -> bool {
        self.start == 0 && self.end == 0
    }
}

pub(crate) type LocalSpan = Span<{ Locality::Local }>;

impl LocalSpan {
    #[allow(dead_code)] // FIXME: actually use it
    pub(crate) fn global(self, file: SourceFileRef<'_>) -> Span {
        self.shift(file.span.start).reinterpret()
    }

    pub(crate) fn range(self) -> std::ops::Range<usize> {
        self.start as usize..self.end as usize
    }
}

#[cfg(test)]
impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

#[derive(PartialEq, Eq, std::marker::ConstParamTy)]
pub(crate) enum Locality {
    Local,
    Global,
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub(crate) struct Spanned<T> {
    pub(crate) span: Span,
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
