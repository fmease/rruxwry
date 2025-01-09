use crate::utility::monotonic::MonotonicVec;
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(crate) struct SourceMap {
    files: MonotonicVec<SourceFile>,
}

impl SourceMap {
    fn offset(&self) -> u32 {
        const PADDING: u32 = 1;
        self.files.last().map(|file| file.span.end + PADDING).unwrap_or_default()
    }

    // FIXME: Detect circular/cyclic imports here.
    pub(crate) fn add(&self, path: &Path) -> io::Result<SourceFileIndex> {
        // FIXME: Provide a proper error context.
        // FIXME: On `--force` (hypoth), we could suppress the error and
        //        create a sham/dummy SourceFile.
        let contents = std::fs::read_to_string(path)?;

        let index = self.files.push(SourceFile::new(path.to_owned(), contents, self.offset()));

        Ok(SourceFileIndex(index))
    }

    // FIXME: Use this comment again:
    // UNSAFETY: `project` must return an address to `R` that remains valid even if `T` is moved.

    pub(crate) fn by_span(&self, span: Span) -> SourceFileRef<'_> {
        // FIXME: Perform a binary search by span instead.
        let file = self.files.iter().find(|file| file.span.contains(span.start)).unwrap();
        // FIXME: Dry, safety comment
        SourceFileRef {
            path: unsafe { &*std::ptr::from_ref(file.path.as_path()) },
            contents: unsafe { &*std::ptr::from_ref(file.contents.as_str()) },
            span: file.span,
        }
    }

    pub(crate) fn get(&self, index: SourceFileIndex) -> SourceFileRef<'_> {
        let file = self.files.get(index.0).unwrap();
        // FIXME: Dry, safety comment
        SourceFileRef {
            path: unsafe { &*std::ptr::from_ref(file.path.as_path()) },
            contents: unsafe { &*std::ptr::from_ref(file.contents.as_str()) },
            span: file.span,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SourceFileIndex(usize);

struct SourceFile {
    path: PathBuf,
    contents: String,
    span: Span,
}

impl SourceFile {
    fn new(path: PathBuf, contents: String, offset: u32) -> Self {
        let span = Span { start: offset, end: offset + u32::try_from(contents.len()).unwrap() };
        Self { path, contents, span }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SourceFileRef<'a> {
    pub(crate) path: &'a Path,
    pub(crate) contents: &'a str,
    pub(crate) span: Span<{ Locality::Global }>,
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub(crate) struct Span<const L: Locality = { Locality::Global }> {
    pub(crate) start: u32,
    pub(crate) end: u32,
}

impl<const L: Locality> Span<L> {
    fn contains(self, index: u32) -> bool {
        self.start <= index && index < self.end
    }

    fn unshift(self, offset: u32) -> Self {
        Self { start: self.start - offset, end: self.end - offset }
    }

    pub(crate) fn shift(self, offset: u32) -> Self {
        Self { start: self.start + offset, end: self.end + offset }
    }

    pub(crate) fn reinterpret<const P: Locality>(self) -> Span<P> {
        Span { start: self.start, end: self.end }
    }
}

impl Span {
    pub(crate) fn local(self, file: SourceFileRef<'_>) -> LocalSpan {
        self.unshift(file.span.start).reinterpret()
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

#[derive(PartialEq, Eq, std::marker::ConstParamTy)]
pub(crate) enum Locality {
    Local,
    Global,
}