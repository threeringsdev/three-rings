//! My-cards mode surfaces (specs/app-ui.md). The sidebar collection tree lives
//! in [`tree`], the `/my` everything-view in [`all_cards`], the phone's `/my`
//! drill-down root list in [`root`], the binder/deck view in [`collection`], a
//! collection's needs (and the pull/pick-list flow) in [`needs`], the global
//! shopping list in [`shopping`], the "Recently deleted" list in
//! [`recently_deleted`] (specs/collection-deletion.md → step 5), and the
//! selection tray's batch move — which spans several of those and is hosted by
//! the shell — in [`move_selection`].

pub mod all_cards;
pub mod collection;
pub mod move_selection;
pub mod needs;
pub mod recently_deleted;
pub mod root;
pub mod shopping;
pub mod tree;
pub mod tree_manage;
