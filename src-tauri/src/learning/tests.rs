use super::*;
use crate::attempts::{self, FeedbackMode, SessionSource, SessionStart};
use crate::bank::import_questions;
use crate::bank::question::tests::multiple;
use crate::db::Database;
use rusqlite::params;

/// A question as an attempt stores it; reviews never read it.
const SNAPSHOT: &str = r#"{"type":"multiple","stem":"x","options":[]}"#;
const D0: &str = "2026-11-01T10:00:00.000Z";
const D1: &str = "2026-11-02T10:00:00.000Z";
const D4: &str = "2026-11-05T10:00:00.000Z";
const D5: &str = "2026-11-06T10:00:00.000Z";
const D11: &str = "2026-11-12T10:00:00.000Z";
const D25: &str = "2026-11-26T10:00:00.000Z";

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
    fn id(&self, local: &str) -> String {
        self.db
            .with(|c| {
                Ok(c.query_row(
                    "SELECT id FROM questions WHERE stem = ?1",
                    params![format!("Enunciado {local}")],
                    |r| r.get::<_, String>(0),
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
    fn finished_attempt(&self, at: &str, responses: &[(&str, &str)]) -> String {
        let n = self.next_attempt.get() + 1;
        self.next_attempt.set(n);
        let id = format!("a{n}");
        let rows: Vec<_> = responses
            .iter()
            .map(|(l, c)| (self.id(l), self.option(l, c), *c != "blank"))
            .collect();
        self.db
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO attempts (id, source_kind, feedback_mode, started_at, finished_at)
                     VALUES (?1, 'collection', 'exam', ?2, ?2)",
                    params![id, at],
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

    fn errors(&self, include_blanks: bool) -> Vec<String> {
        self.db
            .with(|c| error_questions(c, include_blanks, 100))
            .unwrap()
    }

    fn due_list(&self, now: &str) -> Vec<String> {
        self.db.with(|c| due_questions(c, now, 100)).unwrap()
    }
}

use rusqlite::OptionalExtension;

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
fn the_first_hit_schedules_one_day_and_the_second_three() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o0")]);
    assert_eq!(b.interval("q1"), Some(1));
    assert_eq!(b.due("q1").as_deref(), Some(D1));
    b.answer(D1, &[("q1", "o0")]);
    assert_eq!(b.interval("q1"), Some(3));
    assert_eq!(b.due("q1").as_deref(), Some(D4));
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
             VALUES ('open', 'collection', 'study', ?1)",
            params![D0],
        )?;
        tx.execute(
            "INSERT INTO responses (attempt_id, position, question_id, snapshot, choice, answered_at)
             VALUES ('open', 0, ?1, '{}', ?2, ?3)",
            params![q1, key, D0],
        )?;
        refresh_attempt(tx, "open")
    })
    .unwrap();
    assert_eq!(b.due("q1"), None);
    assert!(b.errors(true).is_empty());
}

#[test]
fn an_annulled_question_drops_out_and_returns_when_restored() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o1")]);
    assert_eq!(b.errors(false).len(), 1);
    assert!(b.due("q1").is_some());

    let q1 = b.id("q1");
    b.db.transaction(|tx| {
        tx.execute(
            "UPDATE questions SET annulled = 1 WHERE id = ?1",
            params![q1],
        )?;
        refresh_question(tx, &q1)
    })
    .unwrap();
    assert!(b.errors(false).is_empty());
    assert_eq!(b.due("q1"), None);

    b.db.transaction(|tx| {
        tx.execute(
            "UPDATE questions SET annulled = 0 WHERE id = ?1",
            params![q1],
        )?;
        refresh_question(tx, &q1)
    })
    .unwrap();
    assert_eq!(b.errors(false).len(), 1);
    assert_eq!(b.due("q1").as_deref(), Some(D1));
}

#[test]
fn annulling_through_an_attempt_refreshes_the_review() {
    let b = bank(&["q1"]);
    let id = b.finished_attempt(D0, &[("q1", "o1")]);
    b.db.transaction(|tx| refresh_attempt(tx, &id)).unwrap();
    b.db.transaction(|tx| attempts::set_annulled(tx, &id, 0, true, D1).map(|_| ()))
        .unwrap();
    assert_eq!(b.due("q1"), None);
}

#[test]
fn a_key_change_moves_reviews_and_errors() {
    let b = bank(&["q1"]);
    // Answered o1 while the key was o0: a miss.
    b.answer(D0, &[("q1", "o1")]);
    assert_eq!(b.interval("q1"), Some(1));
    assert_eq!(b.errors(false), vec![b.id("q1")]);
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
    // Latest answer (o0) is now wrong: error, interval 1, due the day after it.
    assert_eq!(b.errors(false), vec![q1.clone()]);
    assert_eq!(b.interval("q1"), Some(1));
    assert_eq!(b.due("q1").as_deref(), Some("2026-11-03T10:00:00.000Z"));
}

#[test]
fn the_error_notebook_follows_the_latest_result_and_key_corrections() {
    let b = bank(&["q1", "q2"]);
    b.answer(D0, &[("q1", "o1"), ("q2", "o1")]);
    assert_eq!(b.errors(false).len(), 2);
    // A later hit on q1 removes it; q2 stays.
    b.answer(D1, &[("q1", "o0")]);
    assert_eq!(b.errors(false), vec![b.id("q2")]);
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
    assert!(b.errors(false).is_empty());
}

#[test]
fn blanks_are_in_the_error_notebook_only_on_request() {
    let b = bank(&["q1", "q2"]);
    b.answer(D0, &[("q1", "blank"), ("q2", "o1")]);
    assert_eq!(b.errors(false), vec![b.id("q2")]);
    let mut with_blanks = b.errors(true);
    with_blanks.sort();
    let mut expected = vec![b.id("q1"), b.id("q2")];
    expected.sort();
    assert_eq!(with_blanks, expected);
}

#[test]
fn a_later_blank_makes_the_latest_result_a_blank_not_a_wrong() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o1")]);
    b.answer(D1, &[("q1", "blank")]);
    assert!(b.errors(false).is_empty());
    assert_eq!(b.errors(true).len(), 1);
}

#[test]
fn errors_come_newest_first_and_are_capped() {
    let b = bank(&["q1", "q2", "q3"]);
    b.answer(D0, &[("q1", "o1")]);
    b.answer(D4, &[("q3", "o1")]);
    b.answer(D1, &[("q2", "o1")]);
    assert_eq!(b.errors(false), vec![b.id("q3"), b.id("q2"), b.id("q1")]);
    let capped = b.db.with(|c| error_questions(c, false, 2)).unwrap();
    assert_eq!(capped, vec![b.id("q3"), b.id("q2")]);
}

#[test]
fn due_reviews_come_most_overdue_first_and_exclude_upcoming() {
    let b = bank(&["q1", "q2", "q3"]);
    b.answer(D0, &[("q1", "o1")]); // due D1
    b.answer(D1, &[("q2", "o1")]); // due D1 + 1 day
    b.answer(D25, &[("q3", "o0")]); // due in 1 day from D25: not yet
    assert_eq!(b.due_list(D25), vec![b.id("q1"), b.id("q2")]);
    assert_eq!(b.due_list(D0), Vec::<String>::new());
    let capped = b.db.with(|c| due_questions(c, D25, 1)).unwrap();
    assert_eq!(capped, vec![b.id("q1")]);
}

#[test]
fn sessions_are_capped_at_one_hundred_questions() {
    let locals: Vec<String> = (0..120).map(|i| format!("q{i}")).collect();
    let refs: Vec<&str> = locals.iter().map(String::as_str).collect();
    let b = bank(&refs);
    let responses: Vec<(&str, &str)> = refs.iter().map(|l| (*l, "o1")).collect();
    b.answer(D0, &responses);
    let view =
        b.db.transaction(|tx| {
            attempts::start(
                tx,
                &SessionStart {
                    source: SessionSource::Errors {
                        include_blanks: false,
                    },
                    feedback_mode: FeedbackMode::Study,
                },
                D1,
            )
        })
        .unwrap();
    assert_eq!(view.questions.len(), 100);
}

#[test]
fn finishing_an_attempt_schedules_reviews() {
    let b = bank(&["q1"]);
    let q1 = b.id("q1");
    let view =
        b.db.transaction(|tx| {
            tx.execute(
                "INSERT INTO collections (id, title, kind, created_at)
                 VALUES ('c1', 'C', 'imported', ?1)",
                params![D0],
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
    assert_eq!(b.errors(false), vec![b.id("q1")]);
}

#[test]
fn the_overview_counts_due_upcoming_and_errors_and_shows_no_answers() {
    let b = bank(&["q1", "q2", "q3"]);
    b.answer(D0, &[("q1", "o1")]); // due D1, error
    b.answer(D1, &[("q2", "o0")]); // due D1+1d = D1 + 1, hit
    b.answer(D4, &[("q3", "o0")]); // due D5
    let overview = b.db.with(|c| overview(c, D1, false)).unwrap();
    assert_eq!(overview.due, 1);
    assert_eq!(overview.upcoming, 2);
    assert_eq!(overview.errors, 1);
    assert_eq!(overview.items.len(), 3);
    let first = &overview.items[0];
    assert_eq!(first.question_id, b.id("q1"));
    assert!(first.is_error);
    assert_eq!(first.due_at.as_deref(), Some(D1));
    assert!(first.last_attempt_id.starts_with('a'));

    let json = serde_json::to_string(&overview).unwrap();
    for forbidden in ["answer", "explanation", "stem", "options"] {
        assert!(!json.contains(forbidden), "{forbidden} leaked: {json}");
    }
}

#[test]
fn the_overview_lists_blank_errors_only_when_asked() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "blank")]);
    let without = b.db.with(|c| overview(c, D0, false)).unwrap();
    let with = b.db.with(|c| overview(c, D0, true)).unwrap();
    assert_eq!(without.errors, 0);
    assert_eq!(with.errors, 1);
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
fn a_reviews_session_takes_due_reviews_then_recent_errors() {
    let b = bank(&["q1", "q2", "q3"]);
    b.answer(D0, &[("q1", "o1")]); // error, due D1
    b.answer(D1, &[("q2", "o0")]); // hit, due D1 + 1 day
    b.answer(D4, &[("q3", "o1")]); // error, due D5: not due at D4
    let session = b.db.with(|c| review_session(c, D4, 100)).unwrap();
    // q1 and q2 are due; q3 is not due but is a recent error.
    assert_eq!(session, vec![b.id("q1"), b.id("q2"), b.id("q3")]);
    let capped = b.db.with(|c| review_session(c, D4, 2)).unwrap();
    assert_eq!(capped, vec![b.id("q1"), b.id("q2")]);
    let ov = b.db.with(|c| overview(c, D4, false)).unwrap();
    assert_eq!(ov.practice, 3);
}

#[test]
fn the_overview_list_caps_upcoming_reviews_but_not_their_count() {
    let locals: Vec<String> = (0..60).map(|i| format!("q{i}")).collect();
    let refs: Vec<&str> = locals.iter().map(String::as_str).collect();
    let b = bank(&refs);
    let responses: Vec<(&str, &str)> = refs.iter().map(|l| (*l, "o0")).collect();
    b.answer(D0, &responses);
    let ov = b.db.with(|c| overview(c, D0, false)).unwrap();
    assert_eq!(ov.upcoming, 60);
    assert_eq!(ov.items.len(), MAX_UPCOMING_ROWS);
}

#[test]
fn scoring_in_sql_agrees_with_attempts_outcome() {
    let b = bank(&["q1"]);
    let key = b.option("q1", "o0");
    let wrong = b.option("q1", "o1");
    for choice in [key.as_deref(), wrong.as_deref(), None] {
        let hit = choice == key.as_deref();
        let expected = attempts::outcome(choice, key.as_deref().unwrap(), false);
        assert_eq!(hit, expected == attempts::Outcome::Correct);
    }
    // The replay agrees on a hit, a miss and a blank.
    b.answer(D0, &[("q1", "o0")]);
    assert_eq!(b.interval("q1"), Some(1));
    b.answer(D1, &[("q1", "o1")]);
    assert_eq!(b.errors(false).len(), 1);
    b.answer(D4, &[("q1", "blank")]);
    assert!(b.errors(false).is_empty());
}

impl Bank {
    fn start(&self, source: SessionSource, at: &str) -> AppResult<Vec<String>> {
        let view = self.db.transaction(|tx| {
            attempts::start(
                tx,
                &SessionStart {
                    source,
                    feedback_mode: FeedbackMode::Study,
                },
                at,
            )
        })?;
        Ok(view
            .questions
            .iter()
            .map(|q| q.question_id.clone())
            .collect())
    }
}

#[test]
fn a_second_session_leaves_out_questions_in_an_open_attempt() {
    let b = bank(&["q1", "q2", "q3"]);
    b.answer(D0, &[("q1", "o1"), ("q2", "o1")]); // both wrong, both due D1
    b.answer(D0, &[("q3", "o0")]);
    // An open errors session takes q1 and q2 (not q3, which was a hit).
    let first = b
        .start(
            SessionSource::Errors {
                include_blanks: false,
            },
            D1,
        )
        .unwrap();
    assert_eq!(first.len(), 2);
    // They are still errors and due, but no new session picks them again.
    assert!(b.errors(false).is_empty());
    assert!(b.due_list(D25).iter().all(|id| *id == b.id("q3")));
    let reviews = b.db.with(|c| review_session(c, D25, 100)).unwrap();
    assert_eq!(reviews, vec![b.id("q3")]);
    assert!(
        b.start(
            SessionSource::Errors {
                include_blanks: false
            },
            D1
        )
        .is_err()
    );
}

#[test]
fn finishing_the_open_attempt_frees_its_questions() {
    let b = bank(&["q1"]);
    b.answer(D0, &[("q1", "o1")]);
    b.start(SessionSource::Reviews, D1).unwrap();
    assert!(b.errors(true).is_empty());
    b.db.transaction(|tx| {
        tx.execute(
            "UPDATE attempts SET finished_at = ?1 WHERE finished_at IS NULL",
            params![D4],
        )?;
        Ok(())
    })
    .unwrap();
    // The attempt was finished with the question blank: with blanks on, it is an error again.
    assert_eq!(b.errors(true), vec![b.id("q1")]);
}

#[test]
fn the_overview_still_lists_questions_in_an_open_attempt_but_practice_leaves_them_out() {
    let b = bank(&["q1", "q2"]);
    b.answer(D0, &[("q1", "o1"), ("q2", "o1")]);
    b.start(
        SessionSource::Errors {
            include_blanks: false,
        },
        D1,
    )
    .unwrap();
    let ov = b.db.with(|c| overview(c, D1, false)).unwrap();
    assert_eq!(ov.items.len(), 2);
    assert_eq!(ov.practice, 0);
}
