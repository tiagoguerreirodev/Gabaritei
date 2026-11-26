//! The first-use checklist on Início. Each step is derived from what is in the database, never
//! from a stored flag, so it stays right after a restore or an import from the other computer.

use rusqlite::{Connection, params};
use serde::Serialize;
use ts_rs::TS;

use crate::ai::draft::DraftStatus;
use crate::error::AppResult;

/// One step of the first-use checklist, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FirstStep {
    /// Check the agents in Settings. Done once a generation has produced a package, which
    /// proves the CLIs work (owner decision, 2026-11-26).
    CheckAgents,
    GeneratePackage,
    /// Done once a package was imported or discarded.
    ReviewPackage,
    FinishAttempt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FirstStepStatus {
    pub step: FirstStep,
    pub done: bool,
}

/// The checklist, or an empty list once every step is done (Início then hides it).
pub fn first_steps(conn: &Connection) -> AppResult<Vec<FirstStepStatus>> {
    let generated = exists(conn, "SELECT EXISTS (SELECT 1 FROM drafts)")?;
    let reviewed = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM drafts WHERE status IN (?1, ?2))",
        params![DraftStatus::Imported, DraftStatus::Discarded],
        |row| row.get(0),
    )?;
    let finished = exists(
        conn,
        "SELECT EXISTS (SELECT 1 FROM attempts WHERE finished_at IS NOT NULL)",
    )?;
    let steps = [
        (FirstStep::CheckAgents, generated),
        (FirstStep::GeneratePackage, generated),
        (FirstStep::ReviewPackage, reviewed),
        (FirstStep::FinishAttempt, finished),
    ];
    if steps.iter().all(|(_, done)| *done) {
        return Ok(Vec::new());
    }
    Ok(steps
        .into_iter()
        .map(|(step, done)| FirstStepStatus { step, done })
        .collect())
}

fn exists(conn: &Connection, sql: &str) -> AppResult<bool> {
    Ok(conn.query_row(sql, [], |row| row.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    const T0: &str = "2026-11-01T10:00:00.000Z";

    fn steps_after(sql: &[&str]) -> Vec<(FirstStep, bool)> {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            for statement in sql {
                conn.execute(statement, params![T0])?;
            }
            first_steps(conn)
        })
        .unwrap()
        .into_iter()
        .map(|status| (status.step, status.done))
        .collect()
    }

    fn draft(status: &str) -> String {
        format!(
            "INSERT INTO drafts (id, created_at, status, request, package) \
             VALUES ('d-{status}', ?1, '{status}', '{{}}', '{{}}')"
        )
    }

    const FINISHED_ATTEMPT: &str = "INSERT INTO attempts \
        (id, source_kind, feedback_mode, started_at, finished_at) \
        VALUES ('a1', 'topic', 'study', ?1, ?1)";

    #[test]
    fn a_new_bank_has_every_step_to_do() {
        assert_eq!(
            steps_after(&[]),
            vec![
                (FirstStep::CheckAgents, false),
                (FirstStep::GeneratePackage, false),
                (FirstStep::ReviewPackage, false),
                (FirstStep::FinishAttempt, false),
            ]
        );
    }

    #[test]
    fn a_package_waiting_for_review_checks_the_agents_and_the_generation() {
        assert_eq!(
            steps_after(&[&draft("review")]),
            vec![
                (FirstStep::CheckAgents, true),
                (FirstStep::GeneratePackage, true),
                (FirstStep::ReviewPackage, false),
                (FirstStep::FinishAttempt, false),
            ]
        );
    }

    #[test]
    fn a_discarded_package_counts_as_reviewed() {
        let steps = steps_after(&[&draft("discarded")]);
        assert_eq!(steps[2], (FirstStep::ReviewPackage, true));
    }

    #[test]
    fn an_unfinished_attempt_does_not_finish_the_checklist() {
        let open = "INSERT INTO attempts (id, source_kind, feedback_mode, started_at) \
                    VALUES ('a1', 'topic', 'study', ?1)";
        let steps = steps_after(&[&draft("imported"), open]);
        assert_eq!(steps[3], (FirstStep::FinishAttempt, false));
    }

    #[test]
    fn the_checklist_is_empty_once_every_step_is_done() {
        assert_eq!(steps_after(&[&draft("imported"), FINISHED_ATTEMPT]), vec![]);
    }
}
