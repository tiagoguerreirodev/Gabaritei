use super::*;
use crate::attempts::{self, FeedbackMode, SessionSource, SessionStart, SourceKind};
use crate::bank::catalog::CollectionKind;
use crate::bank::import_questions;
use crate::bank::question::tests::multiple;
use crate::db::Database;
use ErrorFilter::{IncludeBlanks, WrongOnly};
use rand::SeedableRng;
use rusqlite::params;

/// Which latest results count as errors in these tests.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ErrorFilter {
    WrongOnly,
    IncludeBlanks,
}

/// A question as an attempt stores it; reviews never read it.
const SNAPSHOT: &str = r#"{"type":"multiple","stem":"x","options":[]}"#;
const D0: &str = "2026-11-01T10:00:00.000Z";
const D1: &str = "2026-11-02T10:00:00.000Z";
const D4: &str = "2026-11-05T10:00:00.000Z";
const D5: &str = "2026-11-06T10:00:00.000Z";
const D11: &str = "2026-11-12T10:00:00.000Z";

/// Questions `q1..` whose key is option `o0`; tests say `o0` for a hit and `o1` for a miss.
struct Bank {
    db: Database,
    next_attempt: std::cell::Cell<u32>,
}

fn bank(locals: &[&str]) -> Bank {
    let db = Database::open_in_memory().unwrap();
    let inputs: Vec<_> = locals
        .iter()
        .map(|l| multiple(l, &format!("Enunciado {l}"), 0))
        .collect();
    db.transaction(|tx| import_questions(tx, inputs, D0).map(|_| ()))
        .unwrap();
    Bank {
        db,
        next_attempt: 0.into(),
    }
}

impl Bank {
    fn id(&self, local: &str) -> QuestionId {
        self.db
            .with(|c| {
                Ok(c.query_row(
                    "SELECT id FROM questions WHERE stem = ?1",
                    params![format!("Enunciado {local}")],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    fn option(&self, local: &str, symbol: &str) -> Option<String> {
        let id = self.id(local);
        self.db
            .with(|c| {
                let (answer, options): (String, String) = c.query_row(
                    "SELECT answer_id, options FROM questions WHERE id = ?1",
                    params![id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                Ok(match symbol {
                    "blank" => None,
                    "o0" => Some(answer),
                    _ => {
                        let options: Vec<serde_json::Value> =
                            serde_json::from_str(&options).unwrap();
                        options
                            .iter()
                            .map(|o| o["id"].as_str().unwrap().to_owned())
                            .find(|o| *o != answer)
                    }
                })
            })
            .unwrap()
    }

    /// A finished attempt where every response was answered at `at`. Does not refresh reviews.
    fn finished_attempt(&self, at: &str, responses: &[(&str, &str)]) -> AttemptId {
        let n = self.next_attempt.get() + 1;
        self.next_attempt.set(n);
        let id = AttemptId::from(format!("a{n}"));
        let rows: Vec<_> = responses
            .iter()
            .map(|(l, c)| (self.id(l), self.option(l, c), *c != "blank"))
            .collect();
        self.db
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO attempts (id, source_kind, feedback_mode, started_at, finished_at)
                     VALUES (?1, ?2, ?3, ?4, ?4)",
                    params![id, SourceKind::Collection, FeedbackMode::Exam, at],
                )?;
                for (i, (q, choice, answered)) in rows.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO responses (attempt_id, position, question_id, snapshot, choice, answered_at, revealed_at)
                         VALUES (?1, ?2, ?3, ?7, ?4, ?5, ?6)",
                        params![id, i as i64, q, choice, answered.then_some(at), at, SNAPSHOT],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        id
    }

    /// Records an attempt and updates the reviews, the way finishing one does.
    fn answer(&self, at: &str, responses: &[(&str, &str)]) {
        let id = self.finished_attempt(at, responses);
        self.db.transaction(|tx| refresh_attempt(tx, &id)).unwrap();
    }

    fn due(&self, local: &str) -> Option<String> {
        let id = self.id(local);
        self.db
            .with(|c| {
                Ok(c.query_row(
                    "SELECT due_at FROM review_state WHERE question_id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?)
            })
            .unwrap()
    }

    fn interval(&self, local: &str) -> Option<u32> {
        let id = self.id(local);
        self.db
            .with(|c| {
                let state: Option<String> = c
                    .query_row(
                        "SELECT state FROM review_state WHERE question_id = ?1",
                        params![id],
                        |r| r.get(0),
                    )
                    .optional()?;
                Ok(state.map(|s| {
                    serde_json::from_str::<serde_json::Value>(&s).unwrap()["intervalDays"]
                        .as_u64()
                        .unwrap() as u32
                }))
            })
            .unwrap()
    }

    fn errors(&self, filter: ErrorFilter) -> Vec<QuestionId> {
        let latest = self.db.with(|c| latest_results(c)).unwrap();
        latest
            .into_iter()
            .filter(|result| match result.status {
                LatestStatus::Correct => false,
                LatestStatus::Blank => filter == ErrorFilter::IncludeBlanks,
                LatestStatus::Wrong => true,
            })
            .map(|result| result.question_id)
            .collect()
    }
}

use rusqlite::OptionalExtension;

#[test]
fn review_interval_follows_the_streak_and_caps_at_sixty_days() {
    assert_eq!(
        (0..=8).map(interval_after).collect::<Vec<_>>(),
        [1, 1, 3, 7, 14, 30, 60, 60, 60],
    );
}

#[test]
fn intervals_grow_on_hits_along_the_ladder() {
    let b = bank(&["q1"]);
    // 1, 3, 7, 14, 30, 60, then it stays at 60.
    let expected = [
        ("2026-11-01", 1),
        ("2026-11-02", 3),
        ("2026-11-05", 7),
        ("2026-11-12", 14),
        ("2026-11-26", 30),
        ("2026-12-26", 60),
        ("2027-02-25", 60),
    ];
    for (day, interval) in expected {
        b.answer(&format!("{day}T10:00:00.000Z"), &[("q1", "o0")]);
        assert_eq!(b.interval("q1"), Some(interval), "after {day}");
    }
    assert_eq!(b.due("q1").as_deref(), Some("2027-04-26T10:00:00.000Z"));
}

#[test]
fn the_first_hit_schedules_one_day() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o0")]);
    assert_eq!(
        (b.interval("q1"), b.due("q1").as_deref()),
        (Some(1), Some(D1))
    );
}

#[test]
fn the_second_hit_schedules_three_days() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o0")]);
    b.answer(D1, &[("q1", "o0")]);
    assert_eq!(
        (b.interval("q1"), b.due("q1").as_deref()),
        (Some(3), Some(D4))
    );
}

#[test]
fn a_miss_resets_the_interval_to_one_day() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o0")]);
    b.answer(D1, &[("q1", "o0")]);
    b.answer(D4, &[("q1", "o1")]);
    assert_eq!(b.interval("q1"), Some(1));
    assert_eq!(b.due("q1").as_deref(), Some(D5));
    // And the climb starts over from the bottom.
    b.answer(D5, &[("q1", "o0")]);
    assert_eq!(b.interval("q1"), Some(1));
    b.answer(D11, &[("q1", "o0")]);
    assert_eq!(b.interval("q1"), Some(3));
}

#[test]
fn a_blank_counts_as_a_miss() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o0")]);
    b.answer(D1, &[("q1", "o0")]);
    b.answer(D4, &[("q1", "blank")]);
    assert_eq!(b.interval("q1"), Some(1));
}

#[test]
fn refreshing_twice_does_not_create_duplicate_events() {
    let b = bank(&["q1"]);
    let a1 = b.finished_attempt(D0, &[("q1", "o0")]);
    let a2 = b.finished_attempt(D1, &[("q1", "o0")]);
    for _ in 0..3 {
        b.db.transaction(|tx| refresh_attempt(tx, &a1)).unwrap();
        b.db.transaction(|tx| refresh_attempt(tx, &a2)).unwrap();
    }
    assert_eq!(b.interval("q1"), Some(3));
    assert_eq!(b.due("q1").as_deref(), Some(D4));
}

#[test]
fn a_late_event_does_not_override_a_newer_review() {
    let b = bank(&["q1"]);
    // The newest result (a miss on day 5) is recorded first; two older hits arrive afterwards,
    // like an old attempt finished late or a correction applied later.
    let newest = b.finished_attempt(D5, &[("q1", "o1")]);
    b.db.transaction(|tx| refresh_attempt(tx, &newest)).unwrap();
    let older_1 = b.finished_attempt(D0, &[("q1", "o0")]);
    b.db.transaction(|tx| refresh_attempt(tx, &older_1))
        .unwrap();
    let older_2 = b.finished_attempt(D1, &[("q1", "o0")]);
    b.db.transaction(|tx| refresh_attempt(tx, &older_2))
        .unwrap();

    assert_eq!(b.interval("q1"), Some(1));
    assert_eq!(b.due("q1").as_deref(), Some("2026-11-07T10:00:00.000Z"));
}

#[test]
fn unfinished_attempts_do_not_schedule_reviews() {
    let b = bank(&["q1"]);
    let q1 = b.id("q1");
    let key = b.option("q1", "o0");
    b.db.transaction(|tx| {
        tx.execute(
            "INSERT INTO attempts (id, source_kind, feedback_mode, started_at)
             VALUES ('open', ?1, ?2, ?3)",
            params![SourceKind::Collection, FeedbackMode::Study, D0],
        )?;
        tx.execute(
            "INSERT INTO responses (attempt_id, position, question_id, snapshot, choice, answered_at)
             VALUES ('open', 0, ?1, '{}', ?2, ?3)",
            params![q1, key, D0],
        )?;
        refresh_attempt(tx, &AttemptId::from("open"))
    })
    .unwrap();
    assert_eq!(b.due("q1"), None);
    assert!(b.errors(IncludeBlanks).is_empty());
}

fn update_annulment(b: &Bank, local: &str, annulled: bool) {
    let id = b.id(local);
    b.db.transaction(|tx| {
        tx.execute(
            "UPDATE questions SET annulled = ?2 WHERE id = ?1",
            params![id, annulled],
        )?;
        refresh_question(tx, &id)
    })
    .unwrap();
}

#[test]
fn annulling_a_question_removes_the_review() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o1")]);
    assert!(b.due("q1").is_some());

    update_annulment(&b, "q1", true);

    assert_eq!(b.due("q1"), None);
}

#[test]
fn annulling_a_question_removes_it_from_the_error_notebook() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o1")]);
    assert_eq!(b.errors(WrongOnly), [b.id("q1")]);

    update_annulment(&b, "q1", true);

    assert!(b.errors(WrongOnly).is_empty());
}

#[test]
fn restoring_a_question_replays_the_review_history() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o1")]);
    update_annulment(&b, "q1", true);

    update_annulment(&b, "q1", false);

    assert_eq!(b.due("q1").as_deref(), Some(D1));
}

#[test]
fn restoring_a_question_returns_it_to_the_error_notebook() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o1")]);
    update_annulment(&b, "q1", true);

    update_annulment(&b, "q1", false);

    assert_eq!(b.errors(WrongOnly), [b.id("q1")]);
}

#[test]
fn annulling_through_an_attempt_refreshes_the_review() {
    let b = bank(&["q1"]);
    let id = b.finished_attempt(D0, &[("q1", "o1")]);
    b.db.transaction(|tx| refresh_attempt(tx, &id)).unwrap();
    b.db.transaction(|tx| attempts::annul(tx, &id, 0, D1).map(|_| ()))
        .unwrap();
    assert_eq!(b.due("q1"), None);
}

fn change_key_after_two_answers(b: &Bank) {
    // Answered o1 while the key was o0: a miss.
    b.answer(D0, &[("q1", "o1")]);
    assert_eq!(b.interval("q1"), Some(1));
    assert_eq!(b.errors(WrongOnly), [b.id("q1")]);
    // A hit afterwards on o0 starts a streak at the first rung.
    b.answer(D1, &[("q1", "o0")]);
    assert_eq!(b.interval("q1"), Some(1));

    // The re-check moves the key to o1: both answers are re-scored (miss→hit, hit→miss).
    let q1 = b.id("q1");
    let new_key = b.option("q1", "o1").unwrap();
    b.db.transaction(|tx| {
        tx.execute(
            "UPDATE questions SET answer_id = ?2 WHERE id = ?1",
            params![q1, new_key],
        )?;
        refresh_question(tx, &q1)
    })
    .unwrap();
}

#[test]
fn a_key_change_replays_the_review_history() {
    let b = bank(&["q1"]);
    change_key_after_two_answers(&b);
    // The latest answer (o0) is now wrong, with an interval of one day.
    assert_eq!(
        (b.interval("q1"), b.due("q1").as_deref()),
        (Some(1), Some("2026-11-03T10:00:00.000Z"))
    );
}

#[test]
fn a_key_change_updates_the_error_notebook() {
    let b = bank(&["q1"]);
    change_key_after_two_answers(&b);
    assert_eq!(b.errors(WrongOnly), [b.id("q1")]);
}

#[test]
fn the_error_notebook_uses_the_latest_answer() {
    let b = bank(&["q1", "q2"]);
    b.answer(D0, &[("q1", "o1"), ("q2", "o1")]);
    assert_eq!(b.errors(WrongOnly).len(), 2);
    // A later hit on q1 removes it; q2 stays.
    b.answer(D1, &[("q1", "o0")]);
    assert_eq!(b.errors(WrongOnly), vec![b.id("q2")]);
}

#[test]
fn the_error_notebook_uses_a_corrected_key_without_refresh() {
    let b = bank(&["q2"]);
    b.answer(D0, &[("q2", "o1")]);
    assert_eq!(b.errors(WrongOnly), [b.id("q2")]);

    // Correcting q2's key to what was answered clears it, with no refresh needed.
    let q2 = b.id("q2");
    let o1 = b.option("q2", "o1").unwrap();
    b.db.transaction(|tx| {
        tx.execute(
            "UPDATE questions SET answer_id = ?2 WHERE id = ?1",
            params![q2, o1],
        )?;
        Ok(())
    })
    .unwrap();
    assert!(b.errors(WrongOnly).is_empty());
}

#[test]
fn blanks_are_in_the_error_notebook_only_on_request() {
    let b = bank(&["q1", "q2"]);
    b.answer(D0, &[("q1", "blank"), ("q2", "o1")]);
    assert_eq!(b.errors(WrongOnly), vec![b.id("q2")]);
    let mut with_blanks = b.errors(IncludeBlanks);
    with_blanks.sort();
    let mut expected = vec![b.id("q1"), b.id("q2")];
    expected.sort();
    assert_eq!(with_blanks, expected);
}

#[test]
fn a_later_blank_does_not_hide_an_earlier_wrong_answer() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o1")]);
    b.answer(D1, &[("q1", "blank")]);
    assert_eq!(b.errors(WrongOnly), vec![b.id("q1")]);
    assert_eq!(b.errors(IncludeBlanks), vec![b.id("q1")]);
}

#[test]
fn a_blank_after_a_hit_is_an_error_only_when_blanks_are_included() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o0")]);
    b.answer(D1, &[("q1", "blank")]);
    assert!(b.errors(WrongOnly).is_empty());
    assert_eq!(b.errors(IncludeBlanks).len(), 1);
}

#[test]
fn a_hit_after_a_wrong_answer_leaves_the_notebook() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o1")]);
    b.answer(D1, &[("q1", "o0")]);
    b.answer(D4, &[("q1", "blank")]);
    assert!(b.errors(WrongOnly).is_empty());
}

fn error_bank() -> Bank {
    let b = bank(&["q1", "q2", "q3"]);
    b.answer(D0, &[("q1", "o1")]);
    b.answer(D4, &[("q3", "o1")]);
    b.answer(D1, &[("q2", "o1")]);
    b
}

#[test]
fn errors_come_newest_first() {
    let b = error_bank();
    assert_eq!(b.errors(WrongOnly), [b.id("q3"), b.id("q2"), b.id("q1")]);
}

#[test]
fn finishing_an_attempt_schedules_reviews() {
    let b = bank(&["q1"]);
    let q1 = b.id("q1");
    let view =
        b.db.transaction(|tx| {
            tx.execute(
                "INSERT INTO collections (id, title, kind, created_at)
                 VALUES ('c1', 'C', ?1, ?2)",
                params![CollectionKind::Imported, D0],
            )?;
            tx.execute(
                "INSERT INTO collection_items (collection_id, position, question_id)
                 VALUES ('c1', 0, ?1)",
                params![q1],
            )?;
            attempts::start(
                tx,
                &SessionStart {
                    source: SessionSource::Collection {
                        collection_id: "c1".into(),
                    },
                    feedback_mode: FeedbackMode::Exam,
                },
                D0,
                &mut rand::rngs::StdRng::seed_from_u64(2),
            )
        })
        .unwrap();
    let wrong = b.option("q1", "o1").unwrap();
    b.db.transaction(|tx| {
        attempts::answer(tx, &view.id, 0, Some(&wrong), D0)?;
        attempts::finish(tx, &view.id, view.revision + 1, D1).map(|_| ())
    })
    .unwrap();
    assert_eq!(b.interval("q1"), Some(1));
    assert_eq!(b.errors(WrongOnly), vec![b.id("q1")]);
}

#[test]
fn refresh_all_schedules_history_that_has_no_review_yet() {
    let b = bank(&["q1", "q2"]);
    // Attempts finished before reviews existed: nothing refreshed them.
    b.finished_attempt(D0, &[("q1", "o0"), ("q2", "o1")]);
    b.finished_attempt(D1, &[("q1", "o0")]);
    assert_eq!(b.due("q1"), None);
    b.db.transaction(|tx| refresh_all(tx)).unwrap();
    assert_eq!(b.interval("q1"), Some(3));
    assert_eq!(b.interval("q2"), Some(1));
    // Running it again changes nothing.
    b.db.transaction(|tx| refresh_all(tx)).unwrap();
    assert_eq!(b.interval("q1"), Some(3));
}

#[test]
fn refresh_all_drops_reviews_without_history() {
    let b = bank(&["q1"]);
    let q1 = b.id("q1");
    b.db.transaction(|tx| {
        tx.execute(
            "INSERT INTO review_state (question_id, due_at, state) VALUES (?1, ?2, '{}')",
            params![q1, D1],
        )?;
        refresh_all(tx)
    })
    .unwrap();
    assert_eq!(b.due("q1"), None);
}

#[test]
fn refresh_all_drops_stale_reviews_for_annulled_questions() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o1")]);
    let id = b.id("q1");
    b.db.transaction(|tx| {
        tx.execute(
            "UPDATE questions SET annulled = 1 WHERE id = ?1",
            params![id],
        )?;
        refresh_all(tx)
    })
    .unwrap();
    assert_eq!(b.due("q1"), None);
}

#[test]
fn scoring_in_sql_agrees_with_attempts_outcome() {
    let b = bank(&["q1"]);
    let key = b.option("q1", "o0");
    let wrong = b.option("q1", "o1");
    for choice in [key.as_deref(), wrong.as_deref(), None] {
        let hit = choice == key.as_deref();
        let expected = attempts::outcome(
            choice,
            key.as_deref().unwrap(),
            attempts::QuestionStatus::Valid,
        );
        assert_eq!(hit, expected == attempts::Outcome::Correct);
    }
    // The replay agrees on a hit, a miss and a blank.
    b.answer(D0, &[("q1", "o0")]);
    assert_eq!(b.interval("q1"), Some(1));
    b.answer(D1, &[("q1", "o1")]);
    assert_eq!(b.errors(WrongOnly).len(), 1);
    // A later blank resets the review streak but never hides the wrong answer.
    b.answer(D4, &[("q1", "blank")]);
    assert_eq!(b.interval("q1"), Some(1));
    assert_eq!(b.errors(WrongOnly).len(), 1);
}

#[test]
fn a_wrong_then_blank_question_is_dated_at_the_wrong_answer() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o1")]);
    b.answer(D1, &[("q1", "blank")]);
    let latest = b.db.with(|c| latest_results(c)).unwrap();
    assert_eq!(
        latest
            .iter()
            .map(|result| (result.at.as_str(), result.status == LatestStatus::Wrong))
            .collect::<Vec<_>>(),
        [(D0, true)]
    );
}

#[test]
fn a_question_only_ever_left_blank_is_an_error_only_with_blanks() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "blank")]);
    assert!(b.errors(WrongOnly).is_empty());
    assert_eq!(b.errors(IncludeBlanks).len(), 1);
}

#[test]
fn a_key_correction_turns_an_earlier_wrong_answer_right_even_after_a_blank() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o1")]);
    b.answer(D1, &[("q1", "blank")]);
    assert_eq!(b.errors(WrongOnly).len(), 1);
    let wrong = b.option("q1", "o1").unwrap();
    let q1 = b.id("q1");
    b.db.with(|c| {
        c.execute(
            "UPDATE questions SET answer_id = ?1 WHERE id = ?2",
            params![wrong, q1],
        )?;
        Ok(())
    })
    .unwrap();
    assert!(b.errors(WrongOnly).is_empty());
}

#[test]
fn overlapping_attempts_rank_by_when_the_answer_was_given() {
    let b = bank(&["q1"]);
    // a1 was answered first (wrong) but finished last; a2 was answered later (right).
    let a1 = b.finished_attempt(D0, &[("q1", "o1")]);
    b.answer(D1, &[("q1", "o0")]);
    b.db.transaction(|tx| {
        tx.execute(
            "UPDATE attempts SET finished_at = ?1 WHERE id = ?2",
            params![D5, a1],
        )?;
        Ok(())
    })
    .unwrap();
    assert!(b.errors(WrongOnly).is_empty());
}
