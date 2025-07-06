use crate::{
    build::{DebugOptions, Engine, QueryEnginePathError, QueryEngineVersionError},
    data::Version,
    source::SourceMap,
    utility::{HashMap, default},
};
use std::{cell::RefCell, ffi::OsString};

pub(crate) macro new($opts:expr) {{
    super let cx = ContextData::new($opts);
    Context::new(&cx)
}}

// FIXME: Add a Painter to the context.

#[derive(Clone, Copy)]
pub(crate) struct Context<'cx> {
    data: &'cx ContextData,
}

impl<'cx> Context<'cx> {
    #[doc(hidden)] // used internally by macro `new`
    pub(crate) fn new(data: &'cx ContextData) -> Self {
        Self { data }
    }

    pub(crate) fn map(self) -> &'cx SourceMap {
        &self.data.map
    }

    pub(crate) fn opts(self) -> &'cx Options {
        &self.data.opts
    }

    #[doc(hidden)] // used internally by macro `invoke`
    pub(crate) fn store(self) -> &'cx QueryStore {
        &self.data.store
    }
}

#[doc(hidden)] // used internally by macro `new`
pub(crate) struct ContextData {
    map: SourceMap,
    opts: Options,
    store: QueryStore,
}

impl ContextData {
    #[doc(hidden)] // used internally by macro `new`
    pub(crate) fn new(opts: Options) -> Self {
        Self { map: default(), opts, store: default() }
    }
}

/// A subset of `Opts` of which we know it won't change over the program lifetime.
// FIXME: Include other "immutable" opts and use it pervasively throughout the project!
pub(crate) struct Options {
    pub(crate) toolchain: Option<OsString>,
    pub(crate) dbg_opts: DebugOptions,
}

macro_rules! store {
    ($( $name:ident($param:ident: $Input:ty) -> $Output:ty; )+) => {
        #[derive(Default)]
        #[doc(hidden)] // used internally by macro `invoke`
        pub(crate) struct QueryStore {
            $(
                #[doc(hidden)] // used internally by macro `invoke`
                pub(crate) $name: Query<$Input, $Output>
            ),+
        }
    };
}

// FIXME: Smh. provide these from within mod `build`.
store! {
    // FIXME: Smh. return `&'cx str` instead of `String`.
    query_engine_path(engine: Engine) -> Result<String, QueryEnginePathError>;
    // FIXME: Smh. return `&'cx Version<String>` or better yet `Version<&'cx str>` instead of `Version<String>`.
    query_engine_version(engine: Engine) -> Result<Version<String>, QueryEngineVersionError>;
}

pub(crate) macro invoke($cx:ident.$query:ident($input:expr)) {
    invoke(&$cx.store().$query, $query, $input, $cx)
}

#[doc(hidden)] // used internally by macro `invoke`
pub(crate) fn invoke<I, O>(
    query: &Query<I, O>,
    compute: fn(I, Context<'_>) -> O,
    input: I,
    cx: Context<'_>,
) -> O
where
    I: Copy + Eq + std::hash::Hash,
    O: Clone,
{
    if let Some(result) = query.cache.borrow().get(&input) {
        return result.clone();
    }

    query.cache.borrow_mut().entry(input).or_insert(compute(input, cx)).clone()
}

#[doc(hidden)] // used internally by macro `invoke`
pub(crate) struct Query<I, O> {
    cache: RefCell<HashMap<I, O>>,
}

impl<I, O> Default for Query<I, O> {
    fn default() -> Self {
        Self { cache: default() }
    }
}
