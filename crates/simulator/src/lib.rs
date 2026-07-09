pub mod matrix;
pub mod permissions;

pub use matrix::{access_matrix, AccessCell, AccessMatrix, SubjectSpec};
pub use permissions::{can_send, can_view, effective_permissions};
