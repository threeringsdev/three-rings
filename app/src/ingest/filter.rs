//! The POC subset filter (specs/catalog-ingestion.md → Stage 1): a checked-in,
//! deterministic predicate — full sets by code plus a layout menagerie by
//! explicit printing id. Compiled in via `include_str!` so the filtered run
//! works anywhere the binary does.

use std::collections::HashSet;

use serde::Deserialize;
use uuid::Uuid;

use super::IngestError;

const POC_FILTER: &str = include_str!("poc_filter.json");

#[derive(Debug, Deserialize)]
struct FilterFile {
    sets: Vec<String>,
    cards: Vec<CardRef>,
}

#[derive(Debug, Deserialize)]
struct CardRef {
    id: Uuid,
}

#[derive(Debug)]
pub struct PocFilter {
    sets: HashSet<String>,
    cards: HashSet<Uuid>,
}

impl PocFilter {
    pub fn poc() -> Result<Self, IngestError> {
        let file: FilterFile = serde_json::from_str(POC_FILTER)
            .map_err(|e| IngestError::Source(format!("poc_filter.json is invalid: {e}")))?;
        Ok(Self {
            sets: file.sets.into_iter().collect(),
            cards: file.cards.into_iter().map(|c| c.id).collect(),
        })
    }

    pub fn matches(&self, set_code: Option<&str>, id: Uuid) -> bool {
        set_code.is_some_and(|s| self.sets.contains(s)) || self.cards.contains(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_in_filter_parses_and_matches() {
        let f = PocFilter::poc().expect("poc_filter.json parses");
        assert!(f.matches(Some("mh3"), Uuid::nil()), "set match");
        assert!(
            f.matches(
                Some("sld"),
                "d5dfd236-b1da-4552-b94f-ebf6bb9dafdf".parse().unwrap()
            ),
            "explicit id match outside the listed sets"
        );
        assert!(
            !f.matches(Some("znr"), Uuid::nil()),
            "everything else excluded"
        );
    }
}
