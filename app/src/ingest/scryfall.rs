//! Scryfall HTTP surface for the bulk path: the `/bulk-data` metadata list,
//! the unpaginated `/sets` inventory, and rate-limit-free bulk-file downloads
//! from `*.scryfall.io` (specs/catalog-ingestion.md → Source facts).
//!
//! Every `api.scryfall.com` request carries an accurate `User-Agent` and an
//! `Accept` header — Scryfall 403s without them (verified 2026-07-16).

use std::io;
use std::path::Path;

use async_compression::tokio::bufread::GzipDecoder;
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, Lines,
};
use uuid::Uuid;

use super::IngestError;

const API: &str = "https://api.scryfall.com";
const USER_AGENT: &str = concat!(
    "three-rings/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/threeringsdev/three-rings)"
);

/// One `/bulk-data` entry (the fields we use).
#[derive(Debug, Deserialize)]
pub struct BulkFile {
    #[serde(rename = "type")]
    pub kind: String,
    /// RFC 3339; recorded on the run row and used for the bulk-mode gate.
    pub updated_at: String,
    pub download_uri: String,
}

#[derive(Debug, Deserialize)]
struct BulkList {
    data: Vec<BulkFile>,
}

/// One `/sets` entry — deserialized straight off Scryfall's Set object (the
/// field names match data-model's `sets` columns) and serialized back out for
/// the `jsonb_to_recordset` upsert.
#[derive(Debug, Serialize, Deserialize)]
pub struct SetRow {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub set_type: String,
    pub released_at: Option<String>,
    pub card_count: Option<i32>,
    pub icon_svg_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SetList {
    #[serde(default)]
    has_more: bool,
    next_page: Option<String>,
    data: Vec<SetRow>,
}

pub struct Client {
    http: reqwest::Client,
}

impl Client {
    pub fn new() -> Result<Self, IngestError> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::ACCEPT, "application/json".parse().unwrap());
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .build()?;
        Ok(Self { http })
    }

    /// Fetch `/bulk-data` and pick the named file (e.g. `default_cards`).
    pub async fn bulk_file(&self, kind: &str) -> Result<BulkFile, IngestError> {
        let list: BulkList = self
            .http
            .get(format!("{API}/bulk-data"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        list.data
            .into_iter()
            .find(|b| b.kind == kind)
            .ok_or_else(|| IngestError::Source(format!("no bulk file of type {kind:?}")))
    }

    /// Fetch every set. One unpaginated response today (1,045 sets); the
    /// pagination loop is defensive in case that ever changes.
    pub async fn sets(&self) -> Result<Vec<SetRow>, IngestError> {
        let mut url = format!("{API}/sets");
        let mut out = Vec::new();
        loop {
            let page: SetList = self
                .http
                .get(&url)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            out.extend(page.data);
            match (page.has_more, page.next_page) {
                (true, Some(next)) => url = next,
                _ => break,
            }
        }
        Ok(out)
    }

    /// Download a bulk file to `dest` (via a `.part` rename so an interrupted
    /// download is never mistaken for a complete one). Bulk hosts have no rate
    /// limits. `Accept-Encoding: gzip` is sent explicitly (reqwest's gzip
    /// feature is off, so the body is stored exactly as served — compressed
    /// when the CDN honors it, identity otherwise; `bulk_lines` sniffs which).
    pub async fn download(&self, uri: &str, dest: &Path) -> Result<(), IngestError> {
        let part = dest.with_extension("part");
        let mut resp = self
            .http
            .get(uri)
            .header(reqwest::header::ACCEPT_ENCODING, "gzip")
            .send()
            .await?
            .error_for_status()?;
        let mut file = File::create(&part).await?;
        while let Some(chunk) = resp.chunk().await? {
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        tokio::fs::rename(&part, dest).await?;
        Ok(())
    }
}

/// Stream a downloaded bulk file line-by-line with flat memory, sniffing the
/// gzip magic bytes — Scryfall's CDN serves the identity (plain) body to
/// clients that don't advertise gzip, and may serve either depending on the
/// edge, so both are handled (observed 2026-07-16).
pub async fn bulk_lines(
    path: &Path,
) -> io::Result<Lines<BufReader<Box<dyn AsyncRead + Send + Unpin>>>> {
    let mut file = File::open(path).await?;
    let mut magic = [0u8; 2];
    let n = file.read(&mut magic).await?;
    file.rewind().await?;
    let reader: Box<dyn AsyncRead + Send + Unpin> = if n == 2 && magic == [0x1f, 0x8b] {
        let mut gz = GzipDecoder::new(BufReader::new(file));
        gz.multiple_members(true);
        Box::new(gz)
    } else {
        Box::new(file)
    };
    Ok(BufReader::new(reader).lines())
}
