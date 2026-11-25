//! Generated packages waiting for review, stored in the `drafts` table.
//!
//! A [`Draft`] holds the answer keys, so it never leaves the backend: the frontend only ever
//! gets the spoiler-free summary built from it (spec §2 decisions 10 and 11).

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::agent::{Agent, Effort};
use super::blind_check::BlindCheck;
use super::generate::GenerateRequest;
use super::roles::RoleConfig;
use crate::bank::Package;
use crate::error::{AppError, AppResult};
use crate::ids::DraftId;

/// Where a draft is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DraftStatus {
    Review,
    Imported,
    Discarded,
}

crate::enum_text::stored_enum!(DraftStatus {
        Review => "review",
        Imported => "imported",
        Discarded => "discarded",
});

/// Who wrote a package and how long it took (the draft's provenance).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GenerationInfo {
    pub agent: Agent,
    pub model: String,
    pub effort: Effort,
    pub started_at: String,
    pub duration_ms: u64,
    /// Reported by Claude only.
    pub cost_usd: Option<f64>,
}

/// Everything about a draft except the package and the verdicts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftRequest {
    /// The request as made, with attachments reduced to file names.
    pub request: GenerateRequest,
    pub generator: RoleConfig,
    pub checker: RoleConfig,
    pub info: GenerationInfo,
}

/// A generated package and its provenance. Holds answer keys: backend only.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub id: DraftId,
    pub created_at: String,
    pub status: DraftStatus,
    pub request: DraftRequest,
    pub package: Package,
    /// Blind-check verdicts (#17), stored as JSON.
    pub blind_check: Option<BlindCheck>,
}

pub fn insert(conn: &Connection, draft: &Draft) -> AppResult<()> {
    conn.execute(
        "INSERT INTO drafts (id, created_at, status, request, package, blind_check)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            draft.id,
            draft.created_at,
            draft.status,
            serde_json::to_string(&draft.request)?,
            serde_json::to_string(&draft.package)?,
            draft
                .blind_check
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
        ],
    )?;
    Ok(())
}

pub fn load(conn: &Connection, id: &DraftId) -> AppResult<Draft> {
    let row = conn
        .query_row(
            "SELECT created_at, status, request, package, blind_check FROM drafts WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, DraftStatus>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::Invalid("Pacote não encontrado.".into()))?;
    Ok(Draft {
        id: id.to_owned(),
        created_at: row.0,
        status: row.1,
        request: serde_json::from_str(&row.2)?,
        package: serde_json::from_str(&row.3)?,
        blind_check: row.4.map(|text| serde_json::from_str(&text)).transpose()?,
    })
}

pub fn set_blind_check(conn: &Connection, id: &DraftId, check: &BlindCheck) -> AppResult<()> {
    // Only a draft still waiting for review: one that was imported or discarded stays as it is.
    conn.execute(
        "UPDATE drafts SET blind_check = ?2 WHERE id = ?1 AND status = ?3",
        params![id, serde_json::to_string(check)?, DraftStatus::Review],
    )?;
    Ok(())
}

pub fn set_package(conn: &Connection, id: &DraftId, package: &Package) -> AppResult<()> {
    conn.execute(
        "UPDATE drafts SET package = ?2 WHERE id = ?1 AND status = ?3",
        params![id, serde_json::to_string(package)?, DraftStatus::Review],
    )?;
    Ok(())
}

/// Moves a draft out of `review`. Fails if it already left it, so an import or a discard
/// can never be overwritten by a late write.
pub fn set_status(conn: &Connection, id: &DraftId, status: DraftStatus) -> AppResult<()> {
    let changed = conn.execute(
        "UPDATE drafts SET status = ?2 WHERE id = ?1 AND status = ?3",
        params![id, status, DraftStatus::Review],
    )?;
    if changed == 0 {
        return Err(AppError::Invalid(
            "Este pacote já foi importado ou descartado.".into(),
        ));
    }
    Ok(())
}
