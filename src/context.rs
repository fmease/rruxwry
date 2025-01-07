use crate::source::SourceMap;

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
}

#[derive(Default)]
pub(crate) struct ContextData {
    map: SourceMap,
}
