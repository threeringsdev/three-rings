//! My-cards mode surfaces (specs/app-ui.md). The sidebar collection tree lives
//! in [`tree`], the `/my` everything-view in [`all_cards`], the binder/deck
//! view in [`collection`], and the selection tray's batch move — which spans
//! both of those and is hosted by the shell — in [`move_selection`]; the
//! remaining page bodies (needs, shopping) land with their own Stage 3 tasks.

pub mod all_cards;
pub mod collection;
pub mod move_selection;
pub mod tree;
pub mod tree_manage;
