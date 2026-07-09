pub mod diff;
pub mod resolver;
pub mod result;

pub use diff::diff;
pub use resolver::{InMemoryMatchResolver, ResolveResult, ResourceResolver};
pub use result::{
    ChangeOp, ChangedField, DeferredItem, DiffChange, DiffConflict, DiffResult, DiffTarget,
};
