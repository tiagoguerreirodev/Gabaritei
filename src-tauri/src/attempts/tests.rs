use super::*;
use crate::bank::question::tests::{boolean, multiple};
use crate::bank::{TopicPath, import_questions};
use crate::db::Database;
use rand::SeedableRng;

const T0: &str = "2026-11-20T10:00:00.000Z";
const T1: &str = "2026-11-20T10:05:00.000Z";
const T2: &str = "2026-11-20T10:10:00.000Z";
const SECRET: &str = "EXPLICACAO-SECRETA";

/// Three multiple-choice questions and one Certo/Errado, all in collection `c1` and in the
/// default topic. Every question has an explanation and a reference.
fn bank() -> (Database, Vec<QuestionId>) {
    let db = Database::open_in_memory().unwrap();
    let mut inputs = vec![
        multiple("q1", "Primeira", 0),
        multiple("q2", "Segunda", 1),
        multiple("q3", "Terceira", 2),
        boolean("q4", "Quarta", true),
    ];
    for input in &mut inputs {
        input.explanation = format!("{SECRET} {}", input.local_id);
        input.refs = vec!["REF-SECRETA".into()];
    }
    let ids = db
        .transaction(|tx| {
            let outcome = import_questions(tx, inputs, T0)?;
            let map = outcome.ids_by_local_id;
            let ids: Vec<QuestionId> = ["q1", "q2", "q3", "q4"].map(|k| map[k].clone()).into();
            tx.execute(
                "INSERT INTO collections (id, title, kind, created_at)
                 VALUES ('c1', 'Caderno de teste', ?1, ?2)",
                params![crate::bank::catalog::CollectionKind::Imported, T0],
            )?;
            for (i, q) in ids.iter().enumerate() {
                tx.execute(
                    "INSERT INTO collection_items (collection_id, position, question_id)
                     VALUES ('c1', ?1, ?2)",
                    params![i as i64, q],
                )?;
            }
            Ok(ids)
        })
        .unwrap();
    (db, ids)
}

fn start_collection(db: &Database, mode: FeedbackMode) -> AttemptView {
    start_collection_with_seed(db, mode, 0)
}

fn start_collection_with_seed(db: &Database, mode: FeedbackMode, seed: u64) -> AttemptView {
    db.transaction(|tx| {
        start(
            tx,
            &SessionStart {
                source: SessionSource::Collection {
                    collection_id: "c1".into(),
                },
                feedback_mode: mode,
            },
            T0,
            &mut rand::rngs::StdRng::seed_from_u64(seed),
        )
    })
    .unwrap()
}

fn key_of(db: &Database, question_id: &QuestionId) -> String {
    db.with(|c| {
        Ok(c.query_row(
            "SELECT answer_id FROM questions WHERE id = ?1",
            params![question_id],
            |r| r.get(0),
        )?)
    })
    .unwrap()
}

fn wrong_of(db: &Database, view: &ResponseView) -> String {
    let key = key_of(db, &view.question_id);
    view.options
        .iter()
        .find(|o| o.id != key)
        .unwrap()
        .id
        .clone()
}

/// Asserts the serialized question carries nothing that gives the answer away.
fn assert_no_spoiler(view: &ResponseView) {
    let json = serde_json::to_value(view).unwrap();
    assert_eq!(json["reveal"], serde_json::Value::Null, "{json}");
    let text = json.to_string();
    assert!(!text.contains(SECRET), "{text}");
    assert!(!text.contains("REF-SECRETA"), "{text}");
    assert!(!text.contains("answerId"), "{text}");
    assert!(!text.contains("outcome"), "{text}");
}

#[test]
fn scoring_rules() {
    assert_eq!(
        outcome(Some("a"), "a", QuestionStatus::Valid),
        Outcome::Correct
    );
    assert_eq!(
        outcome(Some("b"), "a", QuestionStatus::Valid),
        Outcome::Wrong
    );
    assert_eq!(outcome(None, "a", QuestionStatus::Valid), Outcome::Blank);
    assert_eq!(
        outcome(Some("a"), "a", QuestionStatus::Annulled),
        Outcome::Annulled
    );
    assert_eq!(
        outcome(None, "a", QuestionStatus::Annulled),
        Outcome::Annulled
    );

    let score = Score::of([
        Outcome::Correct,
        Outcome::Wrong,
        Outcome::Blank,
        Outcome::Annulled,
    ]);
    assert_eq!(score.evaluated, 3);
    assert_eq!(score.achievement, Some(1.0 / 3.0));
    assert_eq!(score.accuracy, Some(0.5));
    assert_eq!(Score::of([Outcome::Annulled]).achievement, None);
    assert_eq!(Score::of([Outcome::Blank]).accuracy, None);
}

#[test]
fn unrevealed_questions_carry_no_key_or_explanation_in_either_mode() {
    for mode in [FeedbackMode::Study, FeedbackMode::Exam] {
        let (db, _) = bank();
        let view = start_collection(&db, mode);
        assert_eq!(view.questions.len(), 4);
        view.questions.iter().for_each(assert_no_spoiler);

        // Answer the first question; in Exam it stays hidden, in Study only it is revealed.
        let first = &view.questions[0];
        let choice = wrong_of(&db, first);
        let view = db
            .transaction(|tx| answer(tx, &view.id, 0, Some(&choice), T1))
            .unwrap();
        match mode {
            FeedbackMode::Exam => view.questions.iter().for_each(assert_no_spoiler),
            FeedbackMode::Study => {
                let reveal = view.questions[0].reveal.as_ref().expect("revealed");
                assert_eq!(reveal.outcome, Outcome::Wrong);
                assert!(reveal.explanation.starts_with(SECRET));
                view.questions[1..].iter().for_each(assert_no_spoiler);
            }
        }
        // Through `load` as well (what the `attempt` command returns).
        let loaded = db.with(|c| load(c, &view.id)).unwrap();
        loaded.questions[1..].iter().for_each(assert_no_spoiler);
        assert!(loaded.score.is_none());
    }
}

fn finish_mixed_exam(db: &Database) -> (AttemptView, String) {
    let view = start_collection(db, FeedbackMode::Exam);
    // Answer position 0 right, position 1 wrong, leave the rest blank.
    let right = key_of(db, &view.questions[0].question_id);
    let view = db
        .transaction(|tx| answer(tx, &view.id, 0, Some(&right), T1))
        .unwrap();
    let wrong = wrong_of(db, &view.questions[1]);
    let view = db
        .transaction(|tx| answer(tx, &view.id, 1, Some(&wrong), T1))
        .unwrap();
    let finished = db
        .transaction(|tx| finish(tx, &view.id, view.revision, T2))
        .unwrap();
    (finished, wrong)
}

#[test]
fn finishing_an_exam_reveals_every_question() {
    let (db, _) = bank();
    let (finished, _) = finish_mixed_exam(&db);
    assert_eq!(finished.finished_at.as_deref(), Some(T2));
    assert!(finished.questions.iter().all(|q| q.reveal.is_some()));
}

#[test]
fn finishing_an_exam_scores_choices_and_blanks() {
    let (db, _) = bank();
    let (finished, _) = finish_mixed_exam(&db);
    assert_eq!(
        finished.score,
        Some(Score {
            correct: 1,
            wrong: 1,
            blank: 2,
            annulled: 0,
            evaluated: 4,
            achievement: Some(0.25),
            accuracy: Some(0.5),
        })
    );
}

#[test]
fn correcting_a_key_rescores_a_finished_exam() {
    let (db, ids) = bank();
    let (finished, wrong) = finish_mixed_exam(&db);
    let corrected = &finished.questions[1].question_id;
    db.with(|c| {
        c.execute(
            "UPDATE questions SET answer_id = ?2 WHERE id = ?1",
            params![corrected, wrong],
        )?;
        Ok(())
    })
    .unwrap();
    let reloaded = db.with(|c| load(c, &finished.id)).unwrap();
    assert_eq!(
        reloaded.score,
        Some(Score {
            correct: 2,
            wrong: 0,
            blank: 2,
            annulled: 0,
            evaluated: 4,
            achievement: Some(0.5),
            accuracy: Some(1.0),
        })
    );
    assert!(ids.contains(corrected));
}

#[test]
fn study_mode_locks_a_revealed_choice() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Study);
    let choice = wrong_of(&db, &view.questions[0]);
    let view = db
        .transaction(|tx| answer(tx, &view.id, 0, Some(&choice), T1))
        .unwrap();
    let other = view.questions[0]
        .options
        .iter()
        .find(|o| o.id != choice)
        .unwrap()
        .id
        .clone();

    for result in [
        db.transaction(|tx| answer(tx, &view.id, 0, Some(&other), T2)),
        db.transaction(|tx| answer(tx, &view.id, 0, None, T2)),
        db.transaction(|tx| eliminate(tx, &view.id, 0, &choice)),
    ] {
        let error = result.unwrap_err().to_string();
        assert!(error.contains("já foi revelada"), "{error}");
    }
    let loaded = db.with(|c| load(c, &view.id)).unwrap();
    assert_eq!(loaded.questions[0].choice.as_deref(), Some(choice.as_str()));
    // Flagging a revealed question is still allowed.
    db.transaction(|tx| flag(tx, &view.id, 0, true)).unwrap();
}

#[test]
fn exam_mode_choices_can_change() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Exam);
    let options: Vec<String> = view.questions[0]
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    db.transaction(|tx| answer(tx, &view.id, 0, Some(&options[0]), T1))
        .unwrap();
    let view = db
        .transaction(|tx| answer(tx, &view.id, 0, Some(&options[1]), T1))
        .unwrap();
    assert_eq!(
        view.questions[0].choice.as_deref(),
        Some(options[1].as_str())
    );
}

#[test]
fn exam_mode_choices_can_be_cleared() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Exam);
    let choice = &view.questions[0].options[0].id;
    db.transaction(|tx| answer(tx, &view.id, 0, Some(choice), T1))
        .unwrap();
    let view = db
        .transaction(|tx| answer(tx, &view.id, 0, None, T1))
        .unwrap();
    assert_eq!(view.questions[0].choice, None);
}

#[test]
fn exam_mode_rejects_invalid_choices() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Exam);
    let invalid = db.transaction(|tx| answer(tx, &view.id, 0, Some("nope"), T1));
    assert_eq!(invalid.unwrap_err().to_string(), "Alternativa inválida.");
}

#[test]
fn elimination_toggles_an_option() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Exam);
    let option = view.questions[0].options[0].id.clone();
    let view = db
        .transaction(|tx| eliminate(tx, &view.id, 0, &option))
        .unwrap();
    assert_eq!(view.questions[0].eliminated, vec![option.clone()]);
    let view = db
        .transaction(|tx| eliminate(tx, &view.id, 0, &option))
        .unwrap();
    assert!(view.questions[0].eliminated.is_empty());
}

#[test]
fn eliminating_the_chosen_option_clears_the_choice() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Exam);
    let option = view.questions[0].options[0].id.clone();
    db.transaction(|tx| answer(tx, &view.id, 0, Some(&option), T1))
        .unwrap();
    let view = db
        .transaction(|tx| eliminate(tx, &view.id, 0, &option))
        .unwrap();
    assert_eq!(view.questions[0].choice, None);
    assert_eq!(view.questions[0].eliminated, vec![option]);
}

#[test]
fn choosing_an_eliminated_option_restores_it() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Exam);
    let option = view.questions[0].options[0].id.clone();
    db.transaction(|tx| eliminate(tx, &view.id, 0, &option))
        .unwrap();
    let view = db
        .transaction(|tx| answer(tx, &view.id, 0, Some(&option), T1))
        .unwrap();
    assert_eq!(view.questions[0].choice.as_deref(), Some(option.as_str()));
    assert!(view.questions[0].eliminated.is_empty());
}

#[test]
fn abandoning_leaves_no_history() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Exam);
    let choice = view.questions[0].options[0].id.clone();
    db.transaction(|tx| answer(tx, &view.id, 0, Some(&choice), T1))
        .unwrap();
    db.transaction(|tx| abandon(tx, &view.id)).unwrap();
    let (attempts, responses): (i64, i64) = db
        .with(|c| {
            Ok(c.query_row(
                "SELECT (SELECT COUNT(*) FROM attempts), (SELECT COUNT(*) FROM responses)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!((attempts, responses), (0, 0));
    let detail = db
        .with(|c| crate::bank::catalog::collection(c, &"c1".into()))
        .unwrap();
    assert!(detail.attempts.is_empty());
    assert_eq!(
        db.with(|c| load(c, &view.id)).unwrap_err().to_string(),
        "Tentativa não encontrada."
    );
}

#[test]
fn finishing_rejects_a_stale_revision() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Exam);
    let stale = view.revision;
    let choice = view.questions[0].options[0].id.clone();
    let view = db
        .transaction(|tx| answer(tx, &view.id, 0, Some(&choice), T1))
        .unwrap();
    assert!(view.revision > stale);

    let error = db
        .transaction(|tx| finish(tx, &view.id, stale, T2))
        .unwrap_err()
        .to_string();
    assert!(error.contains("mudou"), "{error}");
    assert!(
        db.with(|c| load(c, &view.id))
            .unwrap()
            .finished_at
            .is_none()
    );
}

#[test]
fn finishing_locks_responses() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Exam);
    let choice = view.questions[0].options[0].id.clone();
    let done = db
        .transaction(|tx| finish(tx, &view.id, view.revision, T2))
        .unwrap();
    for result in [
        db.transaction(|tx| answer(tx, &done.id, 1, Some(&choice), T2)),
        db.transaction(|tx| eliminate(tx, &done.id, 1, &choice)),
        db.transaction(|tx| flag(tx, &done.id, 1, true)),
    ] {
        let error = result.unwrap_err().to_string();
        assert!(error.contains("finalizada"), "{error}");
    }
}

#[test]
fn finished_attempts_cannot_be_abandoned() {
    let (db, _) = bank();
    let (done, _) = finish_mixed_exam(&db);
    let error = db
        .transaction(|tx| abandon(tx, &done.id))
        .unwrap_err()
        .to_string();
    assert!(error.contains("histórico"), "{error}");
}

#[test]
fn finished_attempts_cannot_be_finished_again() {
    let (db, _) = bank();
    let (done, _) = finish_mixed_exam(&db);
    assert!(
        db.transaction(|tx| finish(tx, &done.id, done.revision, T2))
            .is_err()
    );
}

#[test]
fn one_active_attempt_per_collection() {
    let (db, _) = bank();
    let first = start_collection(&db, FeedbackMode::Exam);
    let again = db.transaction(|tx| {
        start(
            tx,
            &SessionStart {
                source: SessionSource::Collection {
                    collection_id: "c1".into(),
                },
                feedback_mode: FeedbackMode::Study,
            },
            T1,
            &mut rand::rngs::StdRng::seed_from_u64(2),
        )
    });
    assert!(again.unwrap_err().to_string().contains("em andamento"));

    db.transaction(|tx| finish(tx, &first.id, first.revision, T1))
        .unwrap();
    let second = start_collection_with_seed(&db, FeedbackMode::Study, 1);
    assert_ne!(second.id, first.id);
    assert_eq!(second.title, "Caderno de teste");
}

fn sample_collection_sessions(db: &Database) -> Vec<AttemptView> {
    (0..30)
        .map(|seed| {
            let view = start_collection_with_seed(db, FeedbackMode::Exam, seed);
            db.transaction(|tx| abandon(tx, &view.id)).unwrap();
            view
        })
        .collect()
}

#[test]
fn sessions_shuffle_the_collection_questions() {
    let (db, mut ids) = bank();
    ids.sort();
    let mut question_orders = std::collections::HashSet::new();
    for view in sample_collection_sessions(&db) {
        let order: Vec<_> = view.questions.into_iter().map(|q| q.question_id).collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, ids);
        question_orders.insert(order);
    }
    assert!(question_orders.len() > 1);
}

#[test]
fn sessions_shuffle_multiple_choice_options() {
    let (db, _) = bank();
    let mut option_orders = std::collections::HashSet::new();
    for view in sample_collection_sessions(&db) {
        for q in view
            .questions
            .into_iter()
            .filter(|q| q.kind == QuestionType::Multiple)
        {
            option_orders.insert(q.options.into_iter().map(|o| o.text).collect::<Vec<_>>());
        }
    }
    assert!(option_orders.len() > 1);
}

#[test]
fn sessions_keep_boolean_options_in_order() {
    let (db, _) = bank();
    for view in sample_collection_sessions(&db) {
        for q in view
            .questions
            .into_iter()
            .filter(|q| q.kind == QuestionType::Boolean)
        {
            let texts: Vec<_> = q.options.into_iter().map(|o| o.text).collect();
            assert_eq!(texts, ["Certo", "Errado"]);
        }
    }
}

#[test]
fn topic_sessions_prefer_unseen_then_least_recent() {
    let (db, ids) = bank();
    let topic_id: TopicId = db
        .with(|c| Ok(c.query_row("SELECT id FROM topics", [], |r| r.get(0))?))
        .unwrap();
    // q1 answered long ago, q2 answered recently; q3 and q4 never answered.
    db.with(|c| {
        c.execute(
            "INSERT INTO attempts (id, source_kind, feedback_mode, started_at, finished_at)
             VALUES ('old', ?1, ?2, ?3, ?3)",
            params![SourceKind::Topic, FeedbackMode::Exam, T0],
        )?;
        c.execute(
            "INSERT INTO responses (attempt_id, position, question_id, snapshot, choice, answered_at)
             VALUES ('old', 0, ?1, '{}', 'x', '2026-01-01T00:00:00.000Z'),
                    ('old', 1, ?2, '{}', 'x', '2026-06-01T00:00:00.000Z')",
            params![ids[0], ids[1]],
        )?;
        Ok(())
    })
    .unwrap();
    let pick = |count| {
        db.transaction(|tx| {
            let view = start(
                tx,
                &SessionStart {
                    source: SessionSource::Topic { topic_id, count },
                    feedback_mode: FeedbackMode::Study,
                },
                T1,
                &mut rand::rngs::StdRng::seed_from_u64(count.into()),
            )?;
            let mut chosen: Vec<QuestionId> = view
                .questions
                .iter()
                .map(|q| q.question_id.clone())
                .collect();
            chosen.sort();
            abandon(tx, &view.id)?;
            Ok(chosen)
        })
        .unwrap()
    };
    let sorted = |list: &[&QuestionId]| {
        let mut v: Vec<QuestionId> = list.iter().map(|s| (*s).clone()).collect();
        v.sort();
        v
    };
    assert_eq!(pick(2), sorted(&[&ids[2], &ids[3]]));
    assert_eq!(pick(3), sorted(&[&ids[2], &ids[3], &ids[0]]));
}

#[test]
fn topic_sessions_use_the_full_topic_path_as_title() {
    let (db, _) = bank();
    let topic_id: TopicId = db
        .with(|c| Ok(c.query_row("SELECT id FROM topics", [], |r| r.get(0))?))
        .unwrap();
    let view = db
        .transaction(|tx| {
            start(
                tx,
                &SessionStart {
                    source: SessionSource::Topic {
                        topic_id,
                        count: 10,
                    },
                    feedback_mode: FeedbackMode::Study,
                },
                T1,
                &mut rand::rngs::StdRng::seed_from_u64(3),
            )
        })
        .unwrap();
    let TopicPath {
        area,
        matter,
        topic,
    } = crate::bank::question::tests::topic();
    assert_eq!(view.title, format!("{area} › {matter} › {topic}"));
}

#[test]
fn topic_sessions_reject_zero_questions() {
    let (db, _) = bank();
    let topic_id: TopicId = db
        .with(|c| Ok(c.query_row("SELECT id FROM topics", [], |r| r.get(0))?))
        .unwrap();
    let bad = db.transaction(|tx| {
        start(
            tx,
            &SessionStart {
                source: SessionSource::Topic { topic_id, count: 0 },
                feedback_mode: FeedbackMode::Study,
            },
            T1,
            &mut rand::rngs::StdRng::seed_from_u64(4),
        )
    });
    assert!(bad.is_err());
}

#[test]
fn empty_sources_are_a_pt_br_error() {
    let (db, _) = bank();
    for source in [
        SessionSource::Errors {
            include_blanks: true,
        },
        SessionSource::Reviews,
    ] {
        let result = db.transaction(|tx| {
            start(
                tx,
                &SessionStart {
                    source: source.clone(),
                    feedback_mode: FeedbackMode::Exam,
                },
                T1,
                &mut rand::rngs::StdRng::seed_from_u64(5),
            )
        });
        assert_eq!(
            result.unwrap_err().to_string(),
            "Não há questões disponíveis para esta sessão."
        );
    }
    let missing = db.transaction(|tx| {
        start(
            tx,
            &SessionStart {
                source: SessionSource::Collection {
                    collection_id: "nope".into(),
                },
                feedback_mode: FeedbackMode::Exam,
            },
            T1,
            &mut rand::rngs::StdRng::seed_from_u64(6),
        )
    });
    assert_eq!(missing.unwrap_err().to_string(), "Caderno não encontrado.");
}

fn finish_exam_with_one_wrong_choice(db: &Database) -> AttemptView {
    let first = start_collection(db, FeedbackMode::Exam);
    let wrong = wrong_of(db, &first.questions[0]);
    let first = db
        .transaction(|tx| answer(tx, &first.id, 0, Some(&wrong), T1))
        .unwrap();
    db.transaction(|tx| finish(tx, &first.id, first.revision, T1))
        .unwrap()
}

fn annul_wrong_answer_in_study(db: &Database, first: &AttemptView) -> (AttemptView, u32) {
    let target = first.questions[0].question_id.clone();
    let second = start_collection_with_seed(db, FeedbackMode::Study, 1);
    let position = second
        .questions
        .iter()
        .position(|q| q.question_id == target)
        .unwrap() as u32;
    let key = key_of(db, &target);
    db.transaction(|tx| answer(tx, &second.id, position, Some(&key), T2))
        .unwrap();
    let second = db
        .transaction(|tx| annul(tx, &second.id, position, T2))
        .unwrap();
    (second, position)
}

#[test]
fn an_unrevealed_study_question_cannot_be_annulled() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Study);
    let early = db.transaction(|tx| annul(tx, &view.id, 0, T2));
    assert!(
        early
            .unwrap_err()
            .to_string()
            .contains("depois de ver o gabarito"),
        "an unrevealed question can't be annulled"
    );
}

#[test]
fn a_study_reveal_shows_annulment() {
    let (db, _) = bank();
    let first = finish_exam_with_one_wrong_choice(&db);
    let (second, position) = annul_wrong_answer_in_study(&db, &first);
    let reveal = second.questions[position as usize].reveal.as_ref().unwrap();
    assert!(reveal.annulled);
    assert_eq!(reveal.outcome, Outcome::Annulled);
}

#[test]
fn annulling_rescores_a_finished_attempt() {
    let (db, _) = bank();
    let first = finish_exam_with_one_wrong_choice(&db);
    let before = first.score.as_ref().unwrap();
    assert_eq!((before.wrong, before.evaluated, before.annulled), (1, 4, 0));
    annul_wrong_answer_in_study(&db, &first);
    let after = db.with(|c| load(c, &first.id)).unwrap().score.unwrap();
    assert_eq!(
        after,
        Score {
            correct: 0,
            wrong: 0,
            blank: 3,
            annulled: 1,
            evaluated: 3,
            achievement: Some(0.0),
            accuracy: None,
        }
    );
}

#[test]
fn topic_sessions_skip_annulled_questions() {
    let (db, mut ids) = bank();
    let first = finish_exam_with_one_wrong_choice(&db);
    annul_wrong_answer_in_study(&db, &first);
    let topic_id: TopicId = db
        .with(|c| Ok(c.query_row("SELECT id FROM topics", [], |r| r.get(0))?))
        .unwrap();
    let topic = db
        .transaction(|tx| {
            start(
                tx,
                &SessionStart {
                    source: SessionSource::Topic {
                        topic_id,
                        count: 10,
                    },
                    feedback_mode: FeedbackMode::Exam,
                },
                T2,
                &mut rand::rngs::StdRng::seed_from_u64(7),
            )
        })
        .unwrap();
    ids.retain(|id| id != &first.questions[0].question_id);
    ids.sort();
    let mut chosen: Vec<_> = topic.questions.into_iter().map(|q| q.question_id).collect();
    chosen.sort();
    assert_eq!(chosen, ids);
}

#[test]
fn unannulling_restores_the_original_score() {
    let (db, _) = bank();
    let first = finish_exam_with_one_wrong_choice(&db);
    let (second, position) = annul_wrong_answer_in_study(&db, &first);
    db.transaction(|tx| restore(tx, &second.id, position, T2))
        .unwrap();
    let restored = db.with(|c| load(c, &first.id)).unwrap();
    assert_eq!(restored.score.unwrap(), first.score.unwrap());
}

#[test]
fn an_answered_exam_question_cannot_be_annulled_before_finish() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Exam);
    let choice = view.questions[0].options[0].id.clone();
    db.transaction(|tx| answer(tx, &view.id, 0, Some(&choice), T1))
        .unwrap();

    for update in [annul, restore] {
        let error = db
            .transaction(|tx| update(tx, &view.id, 0, T1))
            .unwrap_err()
            .to_string();
        assert!(error.contains("depois de ver o gabarito"), "{error}");
    }
    let loaded = db.with(|c| load(c, &view.id)).unwrap();
    loaded.questions.iter().for_each(assert_no_spoiler);
    let annulled: i64 = db
        .with(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM questions WHERE annulled = 1",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(annulled, 0);

    // After finishing, the same question can be annulled.
    let done = db
        .transaction(|tx| finish(tx, &view.id, loaded.revision, T2))
        .unwrap();
    let done = db.transaction(|tx| annul(tx, &done.id, 0, T2)).unwrap();
    assert_eq!(
        done.questions[0].reveal.as_ref().unwrap().outcome,
        Outcome::Annulled
    );
}

#[test]
fn active_lists_unfinished_attempts_newest_first_with_counts_only() {
    let (db, _) = bank();
    let topic_id: TopicId = db
        .with(|c| Ok(c.query_row("SELECT id FROM topics", [], |r| r.get(0))?))
        .unwrap();
    let exam = start_collection(&db, FeedbackMode::Exam);
    let choice = exam.questions[0].options[0].id.clone();
    db.transaction(|tx| answer(tx, &exam.id, 0, Some(&choice), T1))
        .unwrap();
    let topic = db
        .transaction(|tx| {
            start(
                tx,
                &SessionStart {
                    source: SessionSource::Topic { topic_id, count: 2 },
                    feedback_mode: FeedbackMode::Study,
                },
                T2,
                &mut rand::rngs::StdRng::seed_from_u64(8),
            )
        })
        .unwrap();
    // A finished attempt is history, not something to resume.
    db.transaction(|tx| {
            tx.execute(
                "INSERT INTO attempts (id, source_kind, source_ref, feedback_mode, started_at, finished_at)
                 VALUES ('old', ?1, 'c1', ?2, ?3, ?3)",
                params![SourceKind::Collection, FeedbackMode::Study, T0],
            )?;
            Ok(())
        })
        .unwrap();

    let active = db.with(|c| active(c)).unwrap();
    let summary: Vec<_> = active
        .iter()
        .map(|a| (&a.id, a.title.as_str(), a.answered, a.total))
        .collect();
    assert_eq!(
        summary,
        [
            (
                &topic.id,
                "Direito › Constitucional › Controle de constitucionalidade",
                0,
                2
            ),
            (&exam.id, "Caderno de teste", 1, 4),
        ],
        "newest first; the exam counts its answered question but reveals nothing"
    );
    let json = serde_json::to_string(&active).unwrap();
    for secret in [
        SECRET,
        "REF-SECRETA",
        "answerId",
        "explanation",
        "stem",
        "options",
    ] {
        assert!(!json.contains(secret), "{secret} leaked: {json}");
    }
}

#[test]
fn unavailable_topic_references_use_the_generic_session_title() {
    let db = Database::open_in_memory().unwrap();
    let id = AttemptId::from("legacy");
    db.with(|conn| {
        conn.execute(
            "INSERT INTO attempts (id, source_kind, feedback_mode, started_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, SourceKind::Topic, FeedbackMode::Study, T0],
        )?;
        for source_ref in [None, Some("unknown-topic"), Some("42")] {
            conn.execute(
                "UPDATE attempts SET source_ref = ?2 WHERE id = ?1",
                params![id, source_ref],
            )?;
            assert_eq!(load(conn, &id)?.title, "Sessão");
            assert_eq!(active(conn)?[0].title, "Sessão");
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn progress_counts_use_the_saved_choice() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Exam);
    let choice = view.questions[0].options[0].id.as_str();

    for (choice, answered_at, expected) in [(None, Some(T1), 0), (Some(choice), None, 1)] {
        db.with(|conn| {
            conn.execute(
                "UPDATE responses SET choice = ?1, answered_at = ?2
                 WHERE attempt_id = ?3 AND position = 0",
                params![choice, answered_at, view.id],
            )?;
            let active = active(conn)?;
            let collection = crate::bank::catalog::collection(conn, &CollectionId::from("c1"))?;
            assert_eq!(active[0].answered, expected, "resume list");
            assert_eq!(
                collection.attempts[0].answered, expected,
                "collection detail"
            );
            Ok(())
        })
        .unwrap();
    }
}
