use super::*;
use crate::bank::question::tests::{boolean, multiple};
use crate::bank::{TopicPath, import_questions};
use crate::db::Database;

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
                 VALUES ('c1', 'Caderno de teste', 'imported', ?1)",
                params![T0],
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
    assert_eq!(outcome(Some("a"), "a", false), Outcome::Correct);
    assert_eq!(outcome(Some("b"), "a", false), Outcome::Wrong);
    assert_eq!(outcome(None, "a", false), Outcome::Blank);
    assert_eq!(outcome(Some("a"), "a", true), Outcome::Annulled);
    assert_eq!(outcome(None, "a", true), Outcome::Annulled);

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

#[test]
fn finishing_reveals_everything_and_scores_from_the_current_key() {
    let (db, ids) = bank();
    let view = start_collection(&db, FeedbackMode::Exam);
    let mut view = view;
    // Answer position 0 right, position 1 wrong, leave the rest blank.
    let right = key_of(&db, &view.questions[0].question_id);
    view = db
        .transaction(|tx| answer(tx, &view.id, 0, Some(&right), T1))
        .unwrap();
    let wrong = wrong_of(&db, &view.questions[1]);
    view = db
        .transaction(|tx| answer(tx, &view.id, 1, Some(&wrong), T1))
        .unwrap();
    let finished = db
        .transaction(|tx| finish(tx, &view.id, view.revision, T2))
        .unwrap();
    assert_eq!(finished.finished_at.as_deref(), Some(T2));
    assert!(finished.questions.iter().all(|q| q.reveal.is_some()));
    let score = finished.score.clone().unwrap();
    assert_eq!((score.correct, score.wrong, score.blank), (1, 1, 2));

    // A key correction applies retroactively: the wrong answer becomes the key.
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
    let score = reloaded.score.unwrap();
    assert_eq!((score.correct, score.wrong, score.blank), (2, 0, 2));
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
fn exam_mode_choices_can_change_and_clear() {
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
    let view = db
        .transaction(|tx| answer(tx, &view.id, 0, None, T1))
        .unwrap();
    assert_eq!(view.questions[0].choice, None);

    let invalid = db.transaction(|tx| answer(tx, &view.id, 0, Some("nope"), T1));
    assert_eq!(invalid.unwrap_err().to_string(), "Alternativa inválida.");
}

#[test]
fn elimination_is_reversible_and_clears_the_selected_option() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Exam);
    let a = view.questions[0].options[0].id.clone();
    let b = view.questions[0].options[1].id.clone();

    let view = db.transaction(|tx| eliminate(tx, &view.id, 0, &a)).unwrap();
    assert_eq!(view.questions[0].eliminated, vec![a.clone()]);
    let view = db.transaction(|tx| eliminate(tx, &view.id, 0, &a)).unwrap();
    assert!(view.questions[0].eliminated.is_empty());

    // Eliminating the chosen option clears the choice.
    db.transaction(|tx| answer(tx, &view.id, 0, Some(&b), T1))
        .unwrap();
    let view = db.transaction(|tx| eliminate(tx, &view.id, 0, &b)).unwrap();
    assert_eq!(view.questions[0].choice, None);
    assert_eq!(view.questions[0].eliminated, vec![b.clone()]);

    // Choosing an eliminated option brings it back.
    let view = db
        .transaction(|tx| answer(tx, &view.id, 0, Some(&b), T1))
        .unwrap();
    assert_eq!(view.questions[0].choice.as_deref(), Some(b.as_str()));
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
fn finishing_rejects_a_stale_revision_and_locks_the_answers() {
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
    let error = db
        .transaction(|tx| abandon(tx, &done.id))
        .unwrap_err()
        .to_string();
    assert!(error.contains("histórico"), "{error}");
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
        )
    });
    assert!(again.unwrap_err().to_string().contains("em andamento"));

    db.transaction(|tx| finish(tx, &first.id, first.revision, T1))
        .unwrap();
    let second = start_collection(&db, FeedbackMode::Study);
    assert_ne!(second.id, first.id);
    assert_eq!(second.title, "Caderno de teste");
}

#[test]
fn sessions_shuffle_questions_and_multiple_choice_options() {
    let (db, ids) = bank();
    let mut question_orders = std::collections::HashSet::new();
    let mut option_orders = std::collections::HashSet::new();
    for _ in 0..30 {
        let view = start_collection(&db, FeedbackMode::Exam);
        question_orders.insert(
            view.questions
                .iter()
                .map(|q| q.question_id.clone())
                .collect::<Vec<_>>(),
        );
        for q in &view.questions {
            let texts: Vec<&str> = q.options.iter().map(|o| o.text.as_str()).collect();
            match q.kind {
                QuestionType::Boolean => assert_eq!(texts, ["Certo", "Errado"]),
                QuestionType::Multiple => {
                    option_orders.insert(texts.join(","));
                }
            }
        }
        let mut sorted: Vec<QuestionId> = view
            .questions
            .iter()
            .map(|q| q.question_id.clone())
            .collect();
        sorted.sort();
        let mut expected = ids.clone();
        expected.sort();
        assert_eq!(sorted, expected);
        db.transaction(|tx| abandon(tx, &view.id)).unwrap();
    }
    assert!(question_orders.len() > 1);
    assert!(option_orders.len() > 1);
}

#[test]
fn topic_sessions_prefer_unseen_then_least_recent_and_skip_annulled() {
    let (db, ids) = bank();
    let topic_id: TopicId = db
        .with(|c| Ok(c.query_row("SELECT id FROM topics", [], |r| r.get(0))?))
        .unwrap();
    // q4 annulled; q1 answered long ago, q2 answered recently; q3 never answered.
    db.with(|c| {
        c.execute("UPDATE questions SET annulled = 1 WHERE id = ?1", params![ids[3]])?;
        c.execute_batch(&format!(
            "INSERT INTO attempts (id, source_kind, feedback_mode, started_at, finished_at)
               VALUES ('old', 'topic', 'exam', '{T0}', '{T0}');
             INSERT INTO responses (attempt_id, position, question_id, snapshot, choice, answered_at)
               VALUES ('old', 0, '{q1}', '{{}}', 'x', '2026-01-01T00:00:00.000Z'),
                      ('old', 1, '{q2}', '{{}}', 'x', '2026-06-01T00:00:00.000Z');",
            q1 = ids[0],
            q2 = ids[1],
        ))?;
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
            )?;
            let mut chosen: Vec<QuestionId> = view
                .questions
                .iter()
                .map(|q| q.question_id.clone())
                .collect();
            chosen.sort();
            abandon(tx, &view.id)?;
            Ok((chosen, view.title))
        })
        .unwrap()
    };
    let sorted = |list: &[&QuestionId]| {
        let mut v: Vec<QuestionId> = list.iter().map(|s| (*s).clone()).collect();
        v.sort();
        v
    };
    assert_eq!(pick(1).0, sorted(&[&ids[2]]));
    assert_eq!(pick(2).0, sorted(&[&ids[2], &ids[0]]));
    let (all, title) = pick(10);
    assert_eq!(all, sorted(&[&ids[0], &ids[1], &ids[2]]));
    let TopicPath {
        area,
        matter,
        topic,
    } = crate::bank::question::tests::topic();
    assert_eq!(title, format!("{area} › {matter} › {topic}"));

    let bad = db.transaction(|tx| {
        start(
            tx,
            &SessionStart {
                source: SessionSource::Topic { topic_id, count: 0 },
                feedback_mode: FeedbackMode::Study,
            },
            T1,
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
        )
    });
    assert_eq!(missing.unwrap_err().to_string(), "Caderno não encontrado.");
}

#[test]
fn annulling_leaves_every_denominator_and_unannulling_restores_it() {
    let (db, _) = bank();
    // A finished attempt where position 0 was answered wrong.
    let first = start_collection(&db, FeedbackMode::Exam);
    let wrong = wrong_of(&db, &first.questions[0]);
    let target = first.questions[0].question_id.clone();
    let first = db
        .transaction(|tx| answer(tx, &first.id, 0, Some(&wrong), T1))
        .unwrap();
    let first = db
        .transaction(|tx| finish(tx, &first.id, first.revision, T1))
        .unwrap();
    let before = first.score.clone().unwrap();
    assert_eq!((before.wrong, before.evaluated, before.annulled), (1, 4, 0));

    // A Study attempt reveals the same question, then annuls it from there.
    let second = start_collection(&db, FeedbackMode::Study);
    let position = second
        .questions
        .iter()
        .position(|q| q.question_id == target)
        .unwrap() as u32;
    let other = (position + 1) % 4;
    let early = db.transaction(|tx| set_annulled(tx, &second.id, other, true, T2));
    assert!(
        early
            .unwrap_err()
            .to_string()
            .contains("depois de ver o gabarito"),
        "an unrevealed question can't be annulled"
    );
    let key = key_of(&db, &target);
    db.transaction(|tx| answer(tx, &second.id, position, Some(&key), T2))
        .unwrap();
    let second = db
        .transaction(|tx| set_annulled(tx, &second.id, position, true, T2))
        .unwrap();
    let reveal = second.questions[position as usize].reveal.clone().unwrap();
    assert!(reveal.annulled);
    assert_eq!(reveal.outcome, Outcome::Annulled);

    // The earlier, finished attempt is re-scored: the wrong answer leaves the denominator.
    let after = db.with(|c| load(c, &first.id)).unwrap().score.unwrap();
    assert_eq!(
        (after.wrong, after.annulled, after.evaluated),
        (0, 1, 3),
        "{after:?}"
    );
    assert_eq!(after.achievement, Some(0.0));
    assert_eq!(after.accuracy, None);

    // Topic sessions skip it now.
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
            )
        })
        .unwrap();
    assert!(topic.questions.iter().all(|q| q.question_id != target));

    // Unannulling restores everything.
    db.transaction(|tx| set_annulled(tx, &second.id, position, false, T2))
        .unwrap();
    let restored = db.with(|c| load(c, &first.id)).unwrap().score.unwrap();
    assert_eq!(restored, before);
}

#[test]
fn an_answered_exam_question_cannot_be_annulled_before_finish() {
    let (db, _) = bank();
    let view = start_collection(&db, FeedbackMode::Exam);
    let choice = view.questions[0].options[0].id.clone();
    db.transaction(|tx| answer(tx, &view.id, 0, Some(&choice), T1))
        .unwrap();

    for annulled in [true, false] {
        let error = db
            .transaction(|tx| set_annulled(tx, &view.id, 0, annulled, T1))
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
    let done = db
        .transaction(|tx| set_annulled(tx, &done.id, 0, true, T2))
        .unwrap();
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
            )
        })
        .unwrap();
    // A finished attempt is history, not something to resume.
    db.transaction(|tx| {
            tx.execute(
                "INSERT INTO attempts (id, source_kind, source_ref, feedback_mode, started_at, finished_at)
                 VALUES ('old', 'collection', 'c1', 'study', ?1, ?1)",
                params![T0],
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
             VALUES (?1, 'topic', 'study', ?2)",
            params![id, T0],
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
