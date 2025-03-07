use crate::{
    build::{DebugOptions, EngineKind, EngineVersionError, Options, query_engine_version},
    data::Version,
    source::SourceMap,
    utility::default,
};
use std::{cell::RefCell, ffi::OsString};

pub(crate) macro initialize($cx:ident, $opts:expr) {
    let cx = ContextData::new($opts);
    let $cx = Context::new(&cx);
}

// FIXME: Add a Painter to the context.

#[derive(Clone, Copy)]
pub(crate) struct Context<'cx> {
    data: &'cx ContextData,
}

impl<'cx> Context<'cx> {
    pub(crate) fn new(data: &'cx ContextData) -> Self {
        Self { data }
    }

    pub(crate) fn map(self) -> &'cx SourceMap {
        &self.data.map
    }

    // FIXME: Don't clone the version and return `Result<Version<&'cx str>, _>` instead!
    pub(crate) fn engine(self, kind: EngineKind) -> Result<Version<String>, EngineVersionError> {
        let get = |store: &RefCell<Option<Result<_, _>>>| {
            if let Some(result) = &*store.borrow() {
                result.clone()
            } else {
                let result = query_engine_version(
                    kind,
                    self.data.opts.toolchain.as_deref(),
                    self.data.opts.dbg_opts,
                );
                store.borrow_mut().insert(result).clone()
            }
        };

        match kind {
            EngineKind::Rustc => get(&self.data.rustc),
            EngineKind::Rustdoc => get(&self.data.rustdoc),
        }
    }
}
pub(crate) struct ContextData {
    map: SourceMap,
    rustc: RefCell<Option<Result<Version<String>, EngineVersionError>>>,
    rustdoc: RefCell<Option<Result<Version<String>, EngineVersionError>>>,
    opts: MinOpts,
}

impl ContextData {
    pub(crate) fn new(opts: &Options<'_>) -> Self {
        Self {
            map: default(),
            rustc: default(),
            rustdoc: default(),
            opts: MinOpts {
                toolchain: opts.toolchain.map(ToOwned::to_owned),
                dbg_opts: opts.dbg_opts,
            },
        }
    }
}

/// A subset of `Opts` of which we know it won't change over the program lifetime.
struct MinOpts {
    toolchain: Option<OsString>,
    dbg_opts: DebugOptions,
}
