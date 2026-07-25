//! My-cards mode surfaces (specs/app-ui.md). The sidebar collection tree lives
//! in [`tree`], the `/my` everything-view in [`all_cards`], the binder/deck
//! view in [`collection`]; the remaining page bodies (needs, shopping) land
//! with their own Stage 3 tasks.

pub mod all_cards;
pub mod collection;
pub mod tree;
pub mod tree_manage;
