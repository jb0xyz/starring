use serde::Serialize;

use super::model::{PRIVATE_STUDY_ROOM_RECIPE_ID, PRIVATE_STUDY_ROOM_RECIPE_VERSION};

pub(super) const COMPILER_REVISION: u32 = 1;
pub(super) const MAX_COMPILED_REQUIREMENTS: usize = 31;

#[derive(Serialize)]
pub(super) struct RecipeDescriptor {
    pub id: &'static str,
    pub version: u32,
    pub compiler_revision: u32,
    pub max_requirements: usize,
}

pub(super) fn private_study_room_descriptor() -> RecipeDescriptor {
    RecipeDescriptor {
        id: PRIVATE_STUDY_ROOM_RECIPE_ID,
        version: PRIVATE_STUDY_ROOM_RECIPE_VERSION,
        compiler_revision: COMPILER_REVISION,
        max_requirements: 26,
    }
}
