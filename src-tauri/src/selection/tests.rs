use super::*;
use crate::attempts::{FeedbackMode, SourceKind};
use crate::bank::question::tests::multiple;
use crate::bank::{TopicPath, import_questions};
use crate::db::Database;
use StudyStatus::{Blank, Due, Flagged, Unseen, Wrong};
use rand::SeedableRng;
use rusqlite::params;

const NOW: &str = "2026-11-10T10:00:00.000Z";
const TOMORROW: &str = "2026-11-11T10:00:00.000Z";
const T1: &str = "2026-11-01T10:00:00.000Z";
const T2: &str = "2026-11-02T10:00:00.000Z";
const T3: &str = "2026-11-03T10:00:00.000Z";
const STEM: &str = "ENUNCIADO-";
const EXPLANATION: &str = "EXPLICACAO-SECRETA";

struct World {
    db: Database,
}

/// `(local id, topics)` where each topic is `(area, name)`.
fn world(questions: &[(&str, &[(&str, &str)])]) -> World {
    let db = Database::open_in_memory().unwrap();
    let inputs = questions
        .iter()
        .map(|(local, topics)| {
            let mut input = multiple(local, &format!("{STEM}{local}"), 0);
            input.explanation = EXPLANATION.into();
            input.topics = topics
                .iter()
                .map(|(area, name)| TopicPath::new(area, "Matéria", name))
                .collect();
            input
        })
        .collect();
    db.transaction(|tx| import_questions(tx, inputs, T1).map(|_| ()))
        .unwrap();
    World { db }
}

fn simple(locals: &[&str]) -> World {
    let questions: Vec<_> = locals
        .iter()
        .map(|l| (*l, &[("Direito", "A")][..]))
        .collect();
    world(&questions)
}

impl World {
    fn id(&self, local: &str) -> QuestionId {
        self.db
            .with(|c| {
                Ok(c.query_row(
                    "SELECT id FROM questions WHERE stem = ?1",
                    params![format!("{STEM}{local}")],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    /// The stored option id for `"right"` or `"wrong"`; `None` for a blank.
    fn choice(&self, local: &str, kind: &str) -> Option<String> {
        let id = self.id(local);
        let (answer, options): (String, String) = self
            .db
            .with(|c| {
                Ok(c.query_row(
                    "SELECT answer_id, options FROM questions WHERE id = ?1",
                    params![id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        let options: Vec<serde_json::Value> = serde_json::from_str(&options).unwrap();
        let other = options
            .iter()
            .map(|o| o["id"].as_str().unwrap().to_owned())
            .find(|o| *o != answer);
        match kind {
            "right" => Some(answer),
            "wrong" => other,
            _ => None,
        }
    }

    fn topic(&self, name: &str) -> TopicId {
        self.db
            .with(|c| {
                Ok(c.query_row(
                    "SELECT id FROM topics WHERE topic = ?1",
                    params![name],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    fn annul(&self, local: &str) {
        let id = self.id(local);
        self.db
            .with(|c| {
                Ok(c.execute(
                    "UPDATE questions SET annulled = 1 WHERE id = ?1",
                    params![id],
                )?)
            })
            .unwrap();
    }

    /// An attempt with `(local, choice, flagged)` responses, answered at `at`. `choice` is
    /// `"right"`, `"wrong"` or `"blank"`.
    fn attempt(&self, id: &str, at: &str, finished: bool, responses: &[(&str, &str, bool)]) {
        let rows: Vec<_> = responses
            .iter()
            .map(|(local, choice, flagged)| (self.id(local), self.choice(local, choice), *flagged))
            .collect();
        self.db
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO attempts (id, source_kind, feedback_mode, started_at, finished_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, SourceKind::Collection, FeedbackMode::Exam, at, finished.then_some(at)],
                )?;
                for (position, (question, choice, flagged)) in rows.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO responses
                           (attempt_id, position, question_id, snapshot, choice, flagged, answered_at)
                         VALUES (?1, ?2, ?3, '{}', ?4, ?5, ?6)",
                        params![id, position as i64, question, choice, flagged, choice.as_ref().map(|_| at)],
                    )?;
                }
                Ok(())
            })
            .unwrap();
    }

    fn review_due(&self, local: &str, due_at: &str) {
        let id = self.id(local);
        self.db
            .with(|c| {
                Ok(c.execute(
                    "INSERT INTO review_state (question_id, due_at, state) VALUES (?1, ?2, '{}')",
                    params![id, due_at],
                )?)
            })
            .unwrap();
    }

    fn collection(&self, id: &str, locals: &[&str]) {
        let ids: Vec<_> = locals.iter().map(|l| self.id(l)).collect();
        self.db
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO collections (id, title, kind, created_at)
                     VALUES (?1, 'Caderno', 'imported', ?2)",
                    params![id, T1],
                )?;
                for (position, question) in ids.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO collection_items (collection_id, position, question_id)
                         VALUES (?1, ?2, ?3)",
                        params![id, position as i64, question],
                    )?;
                }
                Ok(())
            })
            .unwrap();
    }

    fn summary(&self, filter: &QuestionFilter) -> SelectionSummary {
        self.summary_at(filter, NOW)
    }

    fn summary_at(&self, filter: &QuestionFilter, now: &str) -> SelectionSummary {
        self.db.with(|c| summary(c, filter, now)).unwrap()
    }

    fn matching(&self, filter: &QuestionFilter) -> u32 {
        self.summary(filter).matching
    }

    fn pick(
        &self,
        filter: &QuestionFilter,
        count: u32,
        order: SessionOrder,
    ) -> AppResult<Vec<QuestionId>> {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        self.db
            .with(|c| pick(c, filter, count, order, NOW, &mut rng))
    }

    fn ids(&self, locals: &[&str]) -> Vec<QuestionId> {
        locals.iter().map(|l| self.id(l)).collect()
    }
}

fn statuses(statuses: &[StudyStatus]) -> QuestionFilter {
    QuestionFilter {
        statuses: statuses.to_vec(),
        ..QuestionFilter::default()
    }
}

#[test]
fn an_empty_filter_matches_every_valid_question() {
    let world = simple(&["q1", "q2", "q3"]);
    world.annul("q3");

    assert_eq!(world.matching(&QuestionFilter::default()), 2);
}

#[test]
fn topics_combine_with_or() {
    let world = world(&[
        ("q1", &[("Direito", "A")]),
        ("q2", &[("Direito", "B")]),
        ("q3", &[("Direito", "C")]),
    ]);
    let filter = QuestionFilter {
        topic_ids: vec![world.topic("A"), world.topic("B")],
        ..QuestionFilter::default()
    };

    assert_eq!(world.matching(&filter), 2);
}

#[test]
fn a_question_under_two_topics_counts_once() {
    let world = world(&[("q1", &[("Direito", "A"), ("Direito", "B")])]);
    let filter = QuestionFilter {
        topic_ids: vec![world.topic("A"), world.topic("B")],
        ..QuestionFilter::default()
    };

    assert_eq!(world.matching(&filter), 1);
}

#[test]
fn statuses_combine_with_or_and_topics_with_and() {
    let world = world(&[
        ("wrong_a", &[("Direito", "A")]),
        ("unseen_a", &[("Direito", "A")]),
        ("wrong_b", &[("Direito", "B")]),
    ]);
    world.attempt(
        "a1",
        T1,
        true,
        &[("wrong_a", "wrong", false), ("wrong_b", "wrong", false)],
    );
    let in_a = |statuses: &[StudyStatus]| QuestionFilter {
        topic_ids: vec![world.topic("A")],
        statuses: statuses.to_vec(),
        ..QuestionFilter::default()
    };

    assert_eq!(world.matching(&in_a(&[Wrong, Unseen])), 2);
    assert_eq!(world.matching(&in_a(&[Wrong])), 1);
}

#[test]
fn a_later_blank_keeps_an_earlier_wrong_answer_wrong() {
    let world = simple(&["q1"]);
    world.attempt("a1", T1, true, &[("q1", "wrong", false)]);
    world.attempt("a2", T2, true, &[("q1", "blank", false)]);

    assert_eq!(world.matching(&statuses(&[Wrong])), 1);
    assert_eq!(world.matching(&statuses(&[Blank])), 0);
}

#[test]
fn due_uses_now() {
    let world = simple(&["q1"]);
    world.review_due("q1", "2026-11-10T20:00:00.000Z");

    assert_eq!(world.summary_at(&statuses(&[Due]), NOW).matching, 0);
    assert_eq!(world.summary_at(&statuses(&[Due]), TOMORROW).matching, 1);
}

#[test]
fn flagged_reads_the_latest_attempt() {
    let world = simple(&["q1", "q2"]);
    world.attempt(
        "a1",
        T1,
        true,
        &[("q1", "right", true), ("q2", "right", true)],
    );
    world.attempt("a2", T2, true, &[("q1", "right", false)]);

    assert_eq!(world.matching(&statuses(&[Flagged])), 1);
}

#[test]
fn collection_and_attempt_limit_the_set() {
    let world = simple(&["q1", "q2", "q3"]);
    world.collection("c1", &["q1", "q2"]);
    world.attempt("a1", T1, true, &[("q3", "right", false)]);

    let in_collection = QuestionFilter {
        collection_id: Some("c1".into()),
        ..QuestionFilter::default()
    };
    let in_attempt = QuestionFilter {
        attempt_id: Some("a1".into()),
        ..QuestionFilter::default()
    };

    assert_eq!(world.matching(&in_collection), 2);
    assert_eq!(world.matching(&in_attempt), 1);
}

#[test]
fn counts_held_questions_separately() {
    let world = simple(&["q1", "q2"]);
    world.attempt("open", T1, false, &[("q1", "blank", false)]);

    let summary = world.summary(&QuestionFilter::default());

    assert_eq!((summary.matching, summary.in_open_attempt), (2, 1));
}

fn status_counts(summary: &SelectionSummary) -> Vec<(StudyStatus, u32)> {
    summary
        .by_status
        .iter()
        .map(|s| (s.status, s.questions))
        .collect()
}

#[test]
fn status_counts_ignore_the_status_filter() {
    let world = simple(&["q1", "q2", "q3"]);
    world.attempt(
        "a1",
        T1,
        true,
        &[("q1", "wrong", false), ("q2", "blank", false)],
    );

    let summary = world.summary(&statuses(&[Wrong]));

    assert_eq!(summary.matching, 1);
    assert_eq!(
        status_counts(&summary),
        [(Unseen, 2), (Wrong, 1), (Blank, 1), (Due, 0), (Flagged, 0)]
    );
}

#[test]
fn topic_counts_ignore_the_topic_filter() {
    let world = world(&[
        ("q1", &[("Direito", "A")]),
        ("q2", &[("Direito", "A")]),
        ("q3", &[("Direito", "B")]),
    ]);
    let (a, b) = (world.topic("A"), world.topic("B"));
    let filter = QuestionFilter {
        topic_ids: vec![a],
        ..QuestionFilter::default()
    };

    let summary = world.summary(&filter);

    assert_eq!(
        summary.by_topic,
        [
            TopicCount {
                topic_id: a,
                questions: 2
            },
            TopicCount {
                topic_id: b,
                questions: 1
            },
        ]
    );
}

#[test]
fn areas_follow_the_whole_filter_in_portuguese_order() {
    let world = world(&[
        ("q1", &[("Português", "A")]),
        ("q2", &[("Ética", "B")]),
        ("q3", &[("Direito", "C")]),
    ]);
    let filter = QuestionFilter {
        topic_ids: vec![world.topic("A"), world.topic("B")],
        ..QuestionFilter::default()
    };

    let areas: Vec<_> = world
        .summary(&filter)
        .by_area
        .into_iter()
        .map(|a| a.area)
        .collect();

    assert_eq!(areas, ["Ética", "Português"]);
}

#[test]
fn orders_follow_the_statuses() {
    use SessionOrder::*;

    assert_eq!(SessionOrder::allowed_for(&[]), [UnseenFirst, Random]);
    assert_eq!(
        SessionOrder::allowed_for(&[Due]),
        [UnseenFirst, Random, DueFirst]
    );
    assert_eq!(
        SessionOrder::allowed_for(&[Blank]),
        [UnseenFirst, Random, RecentErrorsFirst]
    );
}

#[test]
fn the_summary_has_no_question_text() {
    let world = simple(&["q1"]);

    let json = serde_json::to_string(&world.summary(&QuestionFilter::default())).unwrap();

    for secret in [STEM, EXPLANATION, "Alfa", "Beta"] {
        assert!(!json.contains(secret), "{secret} leaked into {json}");
    }
}

#[test]
fn unseen_first_puts_never_answered_first() {
    let world = simple(&["old", "recent", "never"]);
    world.attempt("a1", T1, true, &[("old", "right", false)]);
    world.attempt("a2", T3, true, &[("recent", "right", false)]);

    let picked = world
        .pick(&QuestionFilter::default(), 3, SessionOrder::UnseenFirst)
        .unwrap();

    assert_eq!(picked, world.ids(&["never", "old", "recent"]));
}

#[test]
fn due_first_needs_the_due_status() {
    let world = simple(&["q1"]);

    let refused = world.pick(&statuses(&[Wrong]), 5, SessionOrder::DueFirst);

    assert_eq!(
        refused.unwrap_err().detail(),
        "A ordem Revisões primeiro exige o status Revisão vencida."
    );
}

#[test]
fn recent_errors_first_needs_an_error_status() {
    let world = simple(&["q1"]);

    let refused = world.pick(&statuses(&[Due]), 5, SessionOrder::RecentErrorsFirst);

    assert_eq!(
        refused.unwrap_err().detail(),
        "A ordem Mais recentes primeiro exige o status Erradas ou Em branco."
    );
}

#[test]
fn recent_errors_first_puts_the_newest_error_first() {
    let world = simple(&["old", "new", "ok"]);
    world.attempt(
        "a1",
        T1,
        true,
        &[("old", "wrong", false), ("ok", "right", false)],
    );
    world.attempt("a2", T2, true, &[("new", "blank", false)]);

    let picked = world
        .pick(
            &statuses(&[Wrong, Blank]),
            5,
            SessionOrder::RecentErrorsFirst,
        )
        .unwrap();

    assert_eq!(picked, world.ids(&["new", "old"]));
}

#[test]
fn due_or_wrong_questions_are_all_picked_with_due_first() {
    let world = simple(&["due_late", "due_early", "wrong_old", "wrong_new", "fine"]);
    world.attempt(
        "a1",
        T1,
        true,
        &[("wrong_old", "wrong", false), ("fine", "right", false)],
    );
    world.attempt("a2", T2, true, &[("wrong_new", "wrong", false)]);
    world.review_due("due_late", "2026-11-09T10:00:00.000Z");
    world.review_due("due_early", "2026-11-05T10:00:00.000Z");

    let picked = world
        .pick(&statuses(&[Due, Wrong]), 100, SessionOrder::DueFirst)
        .unwrap();

    assert_eq!(picked.len(), 4);
}

#[test]
fn held_questions_are_left_out() {
    let world = simple(&["q1", "q2"]);
    world.attempt("open", T1, false, &[("q1", "blank", false)]);

    let picked = world
        .pick(&QuestionFilter::default(), 5, SessionOrder::Random)
        .unwrap();

    assert_eq!(picked, world.ids(&["q2"]));
}

#[test]
fn count_caps_the_pick() {
    let world = simple(&["q1", "q2", "q3"]);

    let picked = world
        .pick(&QuestionFilter::default(), 2, SessionOrder::Random)
        .unwrap();

    assert_eq!(picked.len(), 2);
}
