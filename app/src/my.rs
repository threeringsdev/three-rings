//! My-cards mode surfaces (specs/app-ui.md). The sidebar collection tree lives
//! in [`tree`], the `/my` everything-view in [`all_cards`]; the remaining page
//! bodies (`/my/collections/:id`, needs, shopping) land with their own Stage 3
//! tasks.

pub mod all_cards;
pub mod tree;
pub mod tree_manage;
