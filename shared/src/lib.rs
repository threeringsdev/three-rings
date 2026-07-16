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

pub use catalog::CatalogCount;
pub use collection::{
    AddHave, AddLine, AddWant, AllCardsRow, AllCardsView, BatchMove, Board, CardRow,
    CollectionKind, CollectionSummary, CollectionView, Condition, DesireLine, Finish, HoldingLine,
    LineResult, MoveItem, MoveReceipt, MoveRequest, NeedLocation, NeedRow, NeedsView,
    NewCollection, Page, Rename, Reorder, Reparent, SetQuantity, ShoppingList, ShoppingRow,
    SuggestedDestination, Teardown, TeardownReceipt,
};

/// The one error type both backends converge on (specs/collection-api.md
/// §Error model). Business-level auth *outcomes* (wrong password, unknown OTP)
/// are not modeled here — those ride their own result enums; `ApiError` is for
/// data-access faults that map cleanly onto an HTTP status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "code", rename_all = "snake_case")]
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
    pub fn from_wire(status: u16, body: Option<ErrorBody>) -> Self {
        let message = body.map(|b| b.message).unwrap_or_default();
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

/// A convenience alias for fallible data-access results.
pub type ApiResult<T> = Result<T, ApiError>;

/// Re-exported so downstream crates can name the id type without depending on
/// `uuid` directly for DTO fields that are ids.
pub type Id = Uuid;
