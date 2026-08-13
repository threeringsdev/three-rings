//! Cross-backend contract types (specs/data-access-backends.md).
//!
//! This crate is the "drift guarantee's home": the request/response DTOs and the
//! single [`ApiError`] enum that both data-access backends map into — the hosted
//! (sqlx) impl from DB/validation errors, the native (HTTPS) impl from the HTTP
//! status + wire body it receives. Because both sides speak these exact types,
//! the two backends cannot drift.
//!
//! It is deliberately platform-neutral: it builds unchanged for the wasm hydrate
//! frontend (which deserializes these DTOs off server-fn responses) and for the
//! native/hosted server. So it holds no sqlx, axum, or tokio. The hosted router
//! maps [`ApiError::http_status`] (a plain `u16`) onto its own status type.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod catalog;
pub mod collection;
/// The catalog query grammar (specs/catalog-search.md). It lives here, not in
/// `app`, because *both* halves of the two-surface UX need it: the hosted
/// backend translates the parsed terms to SQL, and the filter rail — which
/// runs in the browser (wasm) and in SSR under either backend — has to read
/// the query text to fill its widgets and rewrite terms in it. Only the SQL
/// emission stayed behind `hosted`. Pure and dependency-free, per this crate's
/// rules.
pub mod search;
pub mod tags;

pub use catalog::{
    has_back_face, CardDetail, CardFace, CardFaceSummary, CardSummary, CatalogCount,
    OwnershipEntry, PrintingSummary, Ruling, SearchQuery, SearchResults, SetQuery, SetSummary,
};
pub use collection::{
    batch_item_error, batch_item_index, default_language, AddHave, AddLine, AddWant, AllCardsRow,
    AllCardsView, BatchMove, Board, CardLocation, CardRow, CollectionKind, CollectionSummary,
    CollectionTotals, CollectionTree, CollectionTreeRow, CollectionView, Condition,
    DeleteCollectionReceipt, DeleteCollectionReq, DeletedCollectionRow, DesireLine, Finish,
    HaveDisposition, HoldingLine, HoldingMove, LineResult, MoveItem, MoveReceipt, MoveRequest,
    NeedRow, NeedsView, NewCollection, Page, QuickAddKind, QuickAddReceipt, RelocatedDesire,
    Rename, Reorder, Reparent, SetQuantity, ShoppingList, ShoppingRow, SuggestedDestination,
    Teardown, TeardownReceipt, UndoReceipt, WantDisposition,
};
pub use tags::{
    union_color_identity, DeckCommanders, NewTag, RenameTag, SetBoard, Tag, TagAssignment,
    TagScope, TaggedCard,
};

/// The one error type both backends converge on (specs/collection-api.md
/// §Error model). Business-level auth *outcomes* (wrong password, unknown OTP)
/// are not modeled here — those ride their own result enums; `ApiError` is for
/// data-access faults that map cleanly onto an HTTP status.
///
/// **Adjacently tagged (`tag`+`content`), not internally tagged.** Every
/// variant here is a newtype around a bare `String`, and serde_json's
/// internally-tagged representation (`#[serde(tag = "code")]` alone) requires
/// merging the tag into the variant's own serialized *map* — a bare `String`
/// serializes to a JSON string, not a map, so there is nothing to merge into.
/// That combination compiled (the derive doesn't catch it) but panicked at
/// serialize time — `serde_json::to_string`/`to_value` on any variant hit
/// `Error("cannot serialize tagged newtype variant ApiError::Validation
/// containing a string")` — the instant something actually serialized an
/// `ApiError` value directly rather than going through `to_wire`'s
/// hand-built `ErrorBody` (P6-083: `ServerFnError<ApiError>` on the
/// server-fn wire is that something — `leptos_server`'s resource
/// serialization embeds the whole `Result<T, ServerFnError<ApiError>>` for
/// SSR→hydration handoff). `tag`+`content` sidesteps this: the content goes
/// in its own field regardless of what it serializes to, so it works for any
/// inner type. Wire shape: `{"code":"validation","message":"invalid
/// cursor"}` — which happens to match [`ErrorBody`]'s own field names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum ApiError {
    /// Unknown id — 404.
    #[error("not found: {0}")]
    NotFound(String),
    /// Missing / invalid session on a session-scoped endpoint — 401.
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    /// RLS / ownership violation — 403.
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// Uniqueness, reparent cycle, inbox-protected op — 409.
    #[error("conflict: {0}")]
    Conflict(String),
    /// Malformed DTO / bad quantity — 422.
    #[error("validation: {0}")]
    Validation(String),
    /// A keyset `?cursor=` failed to decode (bad base64/JSON, or a foreign
    /// shape) — 422, same status as [`Self::Validation`] but a distinct
    /// variant on purpose (P6-043). The query that produced the page is not
    /// at fault — only the page reference is — and a UI that lumped this into
    /// `Validation` had no way to say so: `describe_error`
    /// (`app/src/catalog.rs`) rendered *any* `Validation` in the search box as
    /// a query-grammar rejection, so a stale `?cursor=` on a perfectly good
    /// query ("bolt") read as "bolt" itself being wrong. Distinguishing the
    /// variant is what lets the UI blame the page reference instead.
    #[error("bad cursor: {0}")]
    BadCursor(String),
    /// DB or downstream failure — 502/500. Carries a human message; the
    /// original cause is logged server-side, never shipped to the client.
    #[error("upstream: {0}")]
    Upstream(String),
}

impl ApiError {
    /// The stable machine code (matches the serde `code` tag).
    pub fn code(&self) -> &'static str {
        match self {
            ApiError::NotFound(_) => "not_found",
            ApiError::Unauthorized(_) => "unauthorized",
            ApiError::Forbidden(_) => "forbidden",
            ApiError::Conflict(_) => "conflict",
            ApiError::Validation(_) => "validation",
            ApiError::BadCursor(_) => "bad_cursor",
            ApiError::Upstream(_) => "upstream",
        }
    }

    /// The HTTP status this variant projects to (specs/collection-api.md).
    /// Returned as a plain `u16` so this crate needs no HTTP dependency; the
    /// hosted router maps it onto its status type.
    pub fn http_status(&self) -> u16 {
        match self {
            ApiError::NotFound(_) => 404,
            ApiError::Unauthorized(_) => 401,
            ApiError::Forbidden(_) => 403,
            ApiError::Conflict(_) => 409,
            ApiError::Validation(_) => 422,
            ApiError::BadCursor(_) => 422,
            ApiError::Upstream(_) => 502,
        }
    }

    /// The human-readable message this variant carries.
    pub fn message(&self) -> &str {
        match self {
            ApiError::NotFound(m)
            | ApiError::Unauthorized(m)
            | ApiError::Forbidden(m)
            | ApiError::Conflict(m)
            | ApiError::Validation(m)
            | ApiError::BadCursor(m)
            | ApiError::Upstream(m) => m,
        }
    }

    /// The wire envelope: `{ "error": { "code", "message" } }`. Both the hosted
    /// router (serializing an error response) and the native client
    /// (deserializing one) speak this shape.
    pub fn to_wire(&self) -> ErrorEnvelope {
        ErrorEnvelope {
            error: ErrorBody {
                code: self.code().to_string(),
                message: self.message().to_string(),
                details: None,
            },
        }
    }

    /// Reconstruct an `ApiError` the native client received: the HTTP status
    /// picks the variant, the wire body supplies the message. Falls back to the
    /// status-implied variant when the body is missing/unparseable, and to
    /// `Upstream` for any status we don't map.
    ///
    /// **`bad_cursor` is read off the body's `code`, not the status**, because
    /// it shares its 422 with [`Self::Validation`] — the status alone cannot
    /// tell them apart. Every other variant still owns its status exclusively,
    /// so they stay on the status-only table below; a missing/foreign body (no
    /// `code` at all) degrades to that table's ordinary `422` reading
    /// (`Validation`), which is the conservative choice — a decode failure
    /// must not fabricate the more specific variant.
    pub fn from_wire(status: u16, body: Option<ErrorBody>) -> Self {
        let is_bad_cursor = body.as_ref().is_some_and(|b| b.code == "bad_cursor");
        let message = body.map(|b| b.message).unwrap_or_default();
        if is_bad_cursor {
            return ApiError::BadCursor(message);
        }
        match status {
            404 => ApiError::NotFound(message),
            401 => ApiError::Unauthorized(message),
            403 => ApiError::Forbidden(message),
            409 => ApiError::Conflict(message),
            422 => ApiError::Validation(message),
            _ => ApiError::Upstream(if message.is_empty() {
                format!("upstream status {status}")
            } else {
                message
            }),
        }
    }
}

/// The wire envelope wrapping an [`ErrorBody`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

/// The error payload: a stable `code`, a human `message`, and optional
/// structured `details` (reserved for field-level validation errors).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    /// Reserved for structured, field-level validation details. `None` today;
    /// collection-api's validation errors may populate it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub details: Option<serde_json::Value>,
}

/// Reconstruct the variant from its own [`Display`](std::fmt::Display) text
/// (P6-083: the server-fn wire's `ServerFnError<ApiError>` custom-error slot
/// round-trips a `CustErr` through `Display`/`FromStr`, not serde — see
/// `server_fn::error::ServerFnErrorEncoding`). Every arm strips the exact
/// prefix its own `#[error(...)]` attribute writes above, so this is the
/// left inverse of `Display` for text this crate produced. It is not a
/// general parser: an upstream-composed message that happens to start with
/// one of these prefixes would misclassify, which is the same fidelity the
/// message-prefix convention this replaces already had.
impl std::str::FromStr for ApiError {
    /// Infallible: any unrecognized shape becomes `Upstream` rather than
    /// failing to parse, so a foreign or mangled wire error still reaches
    /// the client as *an* `ApiError` instead of blowing up decoding.
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(if let Some(m) = s.strip_prefix("not found: ") {
            ApiError::NotFound(m.to_string())
        } else if let Some(m) = s.strip_prefix("unauthorized: ") {
            ApiError::Unauthorized(m.to_string())
        } else if let Some(m) = s.strip_prefix("forbidden: ") {
            ApiError::Forbidden(m.to_string())
        } else if let Some(m) = s.strip_prefix("conflict: ") {
            ApiError::Conflict(m.to_string())
        } else if let Some(m) = s.strip_prefix("validation: ") {
            ApiError::Validation(m.to_string())
        } else if let Some(m) = s.strip_prefix("bad cursor: ") {
            ApiError::BadCursor(m.to_string())
        } else if let Some(m) = s.strip_prefix("upstream: ") {
            ApiError::Upstream(m.to_string())
        } else {
            ApiError::Upstream(s.to_string())
        })
    }
}

/// A convenience alias for fallible data-access results.
pub type ApiResult<T> = Result<T, ApiError>;

/// Re-exported so downstream crates can name the id type without depending on
/// `uuid` directly for DTO fields that are ids.
pub type Id = Uuid;

#[cfg(test)]
mod api_error_wire_tests {
    use super::{ApiError, ErrorBody};

    /// P6-083's acceptance sketch: every variant survives `Display` →
    /// `FromStr` — the exact round trip `ServerFnError<ApiError>` performs on
    /// the server-fn wire (`ServerFnErrorEncoding`'s `WrappedServerFn|{Display}`
    /// text, decoded back via `CustErr::from_str`). A corrupt cursor's
    /// `ApiError::BadCursor` (P6-043) must come back `BadCursor`, not degrade
    /// to a generic failure or collapse into `Validation`.
    #[test]
    fn every_variant_round_trips_through_display_and_from_str() {
        for original in [
            ApiError::NotFound("collection".into()),
            ApiError::Unauthorized("invalid token".into()),
            ApiError::Forbidden("not yours".into()),
            ApiError::Conflict("the Inbox cannot be moved".into()),
            ApiError::Validation("unknown term: rareness".into()),
            ApiError::BadCursor("invalid cursor".into()),
            ApiError::Upstream("neon unreachable".into()),
        ] {
            let wire = original.to_string();
            let parsed: ApiError = wire.parse().expect("FromStr is infallible");
            assert_eq!(parsed, original, "round trip of {wire:?}");
        }
    }

    /// Unrecognized text (a foreign error, or `Display` text from a
    /// non-`ApiError` transport failure) must not fail to decode — it
    /// degrades to `Upstream` with the text intact, the same "treat as
    /// breakage worth retrying" rule `components::states::classify` already
    /// applies to prefix-less text.
    #[test]
    fn unrecognized_text_degrades_to_upstream_rather_than_erroring() {
        let parsed: ApiError = "error reaching server".parse().expect("infallible");
        assert_eq!(parsed, ApiError::Upstream("error reaching server".into()));
    }

    /// `serde_json` on the enum **value itself**, not the hand-built
    /// `ErrorBody` — pinning the regression this task's e2e run caught live:
    /// `#[serde(tag = "code")]` alone (internally tagged) compiles for a
    /// newtype-of-`String` variant but panics *at serialize time* the first
    /// time anything actually calls `serde_json::to_string`/`to_value` on
    /// one (`leptos_server`'s resource serialization does exactly that for
    /// `ServerFnError<ApiError>` on the server-fn wire — a corrupt
    /// `?cursor=` on `/catalog` 500'd the whole response with
    /// `net::ERR_EMPTY_RESPONSE` rather than rendering the validation
    /// banner). `to_wire`/`from_wire` never exercised this path — they
    /// hand-build `ErrorBody` — which is how it went unnoticed until a
    /// caller serialized `ApiError` directly.
    #[test]
    fn every_variant_round_trips_through_serde_json_directly() {
        for original in [
            ApiError::NotFound("collection".into()),
            ApiError::Unauthorized("invalid token".into()),
            ApiError::Forbidden("not yours".into()),
            ApiError::Conflict("the Inbox cannot be moved".into()),
            ApiError::Validation("unknown term: rareness".into()),
            ApiError::BadCursor("invalid cursor".into()),
            ApiError::Upstream("neon unreachable".into()),
        ] {
            let json = serde_json::to_string(&original)
                .unwrap_or_else(|e| panic!("serializing {original:?} must not panic: {e}"));
            let parsed: ApiError = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("deserializing {json:?} must not fail: {e}"));
            assert_eq!(parsed, original, "round trip of {json}");
        }
    }

    /// `code()`/`http_status()` for the new P6-043 variant: 422, same as
    /// `Validation`, and its own `bad_cursor` code — the pair the wire and
    /// `from_wire` both lean on to tell the two 422s apart.
    #[test]
    fn bad_cursor_is_422_with_its_own_code() {
        let e = ApiError::BadCursor("invalid cursor".into());
        assert_eq!(e.http_status(), 422);
        assert_eq!(e.code(), "bad_cursor");
        assert_eq!(e.message(), "invalid cursor");
    }

    /// `from_wire`'s disambiguation of the shared 422: the wire `code` picks
    /// `BadCursor` over the status-implied `Validation`, but only when the
    /// body actually says `bad_cursor` — an ordinary validation 422, or a
    /// missing/foreign body, must not be promoted to the more specific
    /// variant it never claimed to be.
    #[test]
    fn from_wire_reads_bad_cursor_off_the_code_not_the_status() {
        let bad_cursor_body = ErrorBody {
            code: "bad_cursor".to_string(),
            message: "invalid cursor".to_string(),
            details: None,
        };
        assert_eq!(
            ApiError::from_wire(422, Some(bad_cursor_body)),
            ApiError::BadCursor("invalid cursor".into())
        );

        let validation_body = ErrorBody {
            code: "validation".to_string(),
            message: "unknown term: rareness".to_string(),
            details: None,
        };
        assert_eq!(
            ApiError::from_wire(422, Some(validation_body)),
            ApiError::Validation("unknown term: rareness".into())
        );

        // No body at all (unparseable envelope) still degrades to the
        // status-implied, conservative reading — never `BadCursor`, which
        // only a `code` can earn.
        assert_eq!(
            ApiError::from_wire(422, None),
            ApiError::Validation(String::new())
        );
    }
}
