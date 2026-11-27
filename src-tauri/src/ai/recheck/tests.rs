#![cfg_attr(not(unix), allow(dead_code, unused_imports))] // fake CLIs need `sh`

use super::*;
use crate::attempts::{
    self, FeedbackMode, Outcome, SessionSource, SessionStart, answer as answer_question, finish,
};
use crate::bank::import_questions;
use crate::bank::question::tests::multiple;
use crate::bank::store::load_question;
use crate::ids::QuestionId;
use rand::SeedableRng;
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const T0: &str = "2026-11-20T10:00:00.000Z";
const T1: &str = "2026-11-20T11:00:00.000Z";

struct World {
    dir: tempfile::TempDir,
    db: Database,
    secrets: Secrets,
    jobs: OnDemand,
    clis: ResolvedClis,
    question_id: QuestionId,
    old_key: String,
    other_option: String,
    rng: std::cell::RefCell<rand::rngs::StdRng>,
}

/// A fake `codex exec` that records its prompt and answers with `answer.json`.
#[cfg(unix)]
fn fake_codex(dir: &Path) -> PathBuf {
    crate::ai::runner::tests::write_script(
        dir,
        "codex",
        &format!(
            r#"dir='{}'
echo x >> "$dir/calls"
out=""
while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift;; esac; shift; done
cat > "$dir/stdin.txt"
cp "$dir/answer.json" "$out""#,
            dir.display()
        ),
    )
}

#[cfg(unix)]
fn world(answer: impl FnOnce(&str, &str) -> Value) -> World {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_in_memory().unwrap();
    let secrets = Secrets::memory();
    let (question_id, old_key, other_option) = db
        .transaction(|tx| {
            let mut input = multiple("q1", "Primeira", 0);
            input.explanation = "COMENTARIO-ATUAL".into();
            let outcome = import_questions(tx, vec![input], T0)?;
            let id = outcome.ids_by_local_id["q1"].clone();
            let question = load_question(tx, &id)?.unwrap();
            let other = question
                .options
                .iter()
                .find(|o| o.id != question.answer_id)
                .unwrap()
                .id
                .clone();
            tx.execute(
                "INSERT INTO collections (id, title, kind, created_at) VALUES ('c1', 'Caderno', ?1, ?2)",
                params![crate::bank::catalog::CollectionKind::Imported, T0],
            )?;
            tx.execute(
                "INSERT INTO collection_items (collection_id, position, question_id) VALUES ('c1', 0, ?1)",
                params![id],
            )?;
            Ok((id, question.answer_id, other))
        })
        .unwrap();
    // The AI names options by letter (or Certo/Errado), never by their internal ids.
    let question = db
        .with(|conn| load_question(conn, &question_id))
        .unwrap()
        .unwrap();
    let label_of = |id: &str| {
        let index = question.options.iter().position(|o| o.id == id).unwrap();
        prompts::option_label(&question, index)
    };
    std::fs::write(
        dir.path().join("answer.json"),
        answer(&label_of(&old_key), &label_of(&other_option)).to_string(),
    )
    .unwrap();
    let cli = fake_codex(dir.path());
    World {
        dir,
        db,
        secrets,
        jobs: OnDemand::default(),
        clis: ResolvedClis::new(None, Some(cli)),
        question_id,
        old_key,
        other_option,
        rng: rand::rngs::StdRng::seed_from_u64(1).into(),
    }
}

fn start(world: &World, mode: FeedbackMode) -> AttemptId {
    world
        .db
        .transaction(|tx| {
            attempts::start(
                tx,
                &SessionStart {
                    source: SessionSource::Collection {
                        collection_id: "c1".into(),
                    },
                    feedback_mode: mode,
                },
                T0,
                &mut *world.rng.borrow_mut(),
            )
        })
        .unwrap()
        .id
}

fn answer_with(world: &World, attempt: &AttemptId, option: &str) {
    world
        .db
        .transaction(|tx| answer_question(tx, attempt, 0, Some(option), T0))
        .unwrap();
}

fn calls(world: &World) -> usize {
    std::fs::read_to_string(world.dir.path().join("calls"))
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

fn ask_it(world: &World, attempt: &AttemptId) -> AppResult<RecheckView> {
    ask_recheck(
        &world.db,
        &world.secrets,
        &world.clis,
        &world.jobs,
        attempt,
        0,
        &CancelToken::new(),
    )
}

fn confirm(world: &World, attempt: &AttemptId) -> AppResult<attempts::AttemptView> {
    world
        .db
        .transaction(|tx| apply(tx, &world.jobs, attempt, 0, T1))
}

fn answered_attempt(world: &World) -> AttemptId {
    let attempt = start(world, FeedbackMode::Study);
    answer_with(world, &attempt, &world.old_key);
    attempt
}

fn finish_attempt(world: &World) -> AttemptId {
    let attempt = answered_attempt(world);
    world
        .db
        .transaction(|tx| finish(tx, &attempt, 1, T0))
        .unwrap();
    attempt
}

fn wrong_answer(_old: &str, new: &str) -> Value {
    json!({ "verdict": "wrong", "correctedKey": new, "reasoning": "O art. 5º diz outra coisa.", "sources": ["CF/88, art. 5º"] })
}

fn outcome_of(world: &World, attempt: &AttemptId) -> Outcome {
    let view = world.db.with(|conn| attempts::load(conn, attempt)).unwrap();
    view.questions[0].reveal.as_ref().unwrap().outcome
}

fn review_interval(world: &World) -> Option<i64> {
    world
        .db
        .with(|conn| {
            Ok(conn
                .query_row(
                    "SELECT json_extract(state, '$.intervalDays') FROM review_state",
                    [],
                    |r| r.get(0),
                )
                .optional()?)
        })
        .unwrap()
}

fn errors_of(world: &World) -> Vec<QuestionId> {
    world
        .db
        .with(|conn| {
            Ok(crate::learning::latest_results(conn)?
                .into_iter()
                .filter(|latest| latest.status == crate::learning::LatestStatus::Wrong)
                .map(|latest| latest.question_id)
                .collect())
        })
        .unwrap()
}

fn mark_disputed(world: &World) {
    world.db.with(|conn| {
        conn.execute(
            "UPDATE questions SET provenance = json_set(provenance, '$.disputed', json('true'), '$.source', ?2, '$.draftId', 'd1') WHERE id = ?1",
            params![world.question_id, "generated"],
        )?;
        Ok(())
    })
    .unwrap();
}

#[cfg(unix)]
mod flow {
    use super::*;

    #[test]
    fn it_is_refused_before_the_answer_is_revealed() {
        let mut w = world(wrong_answer);
        w.clis = ResolvedClis::new(None, None);
        let attempt = start(&w, FeedbackMode::Exam);
        answer_with(&w, &attempt, &w.old_key);
        let error = ask_it(&w, &attempt).unwrap_err();
        assert!(error.to_string().contains("depois de ver o gabarito"));
        assert_eq!(calls(&w), 0, "no AI call happens for a hidden question");
    }

    #[test]
    fn asking_returns_the_proposal_without_changing_the_key() {
        let w = world(wrong_answer);
        let attempt = answered_attempt(&w);

        assert_eq!(
            ask_it(&w, &attempt).unwrap(),
            RecheckView {
                verdict: RecheckVerdict::Wrong,
                corrected_key: Some(w.other_option.clone()),
                reasoning: "O art. 5º diz outra coisa.".into(),
                sources: vec!["CF/88, art. 5º".into()],
            }
        );
        assert_eq!(outcome_of(&w, &attempt), Outcome::Correct);
    }

    #[test]
    fn confirming_a_correction_rescores_past_attempts() {
        let w = world(wrong_answer);
        let first = finish_attempt(&w);
        let attempt = finish_attempt(&w);
        assert_eq!(outcome_of(&w, &attempt), Outcome::Correct);

        ask_it(&w, &attempt).unwrap();
        confirm(&w, &attempt).unwrap();

        assert_eq!(outcome_of(&w, &first), Outcome::Wrong);
        assert_eq!(outcome_of(&w, &attempt), Outcome::Wrong);
    }

    #[test]
    fn confirming_a_correction_refreshes_the_review_interval() {
        let w = world(wrong_answer);
        // Two hits in a row put the review on the second rung.
        finish_attempt(&w);
        let attempt = finish_attempt(&w);
        assert_eq!(review_interval(&w), Some(3), "two hits move the review up");

        ask_it(&w, &attempt).unwrap();
        confirm(&w, &attempt).unwrap();

        assert_eq!(review_interval(&w), Some(1), "both answers are now misses");
    }

    #[test]
    fn confirming_a_correction_refreshes_the_error_notebook() {
        let w = world(wrong_answer);
        let attempt = finish_attempt(&w);
        assert!(errors_of(&w).is_empty());

        ask_it(&w, &attempt).unwrap();
        confirm(&w, &attempt).unwrap();

        assert_eq!(errors_of(&w), std::slice::from_ref(&w.question_id));
    }

    #[test]
    fn confirming_a_correction_records_the_key_change() {
        let w = world(wrong_answer);
        mark_disputed(&w);
        let attempt = answered_attempt(&w);
        ask_it(&w, &attempt).unwrap();
        confirm(&w, &attempt).unwrap();

        let audit: (String, String, String, String) = w
            .db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT old_answer_id, new_answer_id, reason, source FROM key_changes WHERE question_id = ?1",
                    params![w.question_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?)
            })
            .unwrap();
        assert_eq!(
            audit,
            (
                w.old_key,
                w.other_option,
                "O art. 5º diz outra coisa.".into(),
                "recheck:codex/gpt-6.2-sol".into(),
            )
        );
    }

    #[test]
    fn the_recheck_prompt_uses_the_revealed_answer() {
        let w = world(wrong_answer);
        let attempt = start(&w, FeedbackMode::Study);
        answer_with(&w, &attempt, &w.old_key);
        ask_it(&w, &attempt).unwrap();
        let sent = std::fs::read_to_string(w.dir.path().join("stdin.txt")).unwrap();
        assert!(sent.contains("COMENTARIO-ATUAL"));
        let question =
            w.db.with(|conn| load_question(conn, &w.question_id))
                .unwrap()
                .unwrap();
        let key_index = question
            .options
            .iter()
            .position(|o| o.id == w.old_key)
            .unwrap();
        let label = prompts::option_label(&question, key_index);
        assert!(sent.contains(&format!("Gabarito atual: ({label})")));
        // Letters, never the hex ids the user never sees.
        assert!(sent.contains("(A) ") && !sent.contains(&w.old_key));
        assert!(sent.contains("NUNCA invente"));
    }

    #[test]
    fn confirming_a_correction_clears_the_disputed_flag() {
        let w = world(wrong_answer);
        mark_disputed(&w);
        let attempt = answered_attempt(&w);
        ask_it(&w, &attempt).unwrap();
        confirm(&w, &attempt).unwrap();
        let provenance: String =
            w.db.with(|conn| {
                Ok(conn.query_row(
                    "SELECT provenance FROM questions WHERE id = ?1",
                    params![w.question_id],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        let provenance: Value = serde_json::from_str(&provenance).unwrap();
        assert!(provenance.get("disputed").is_none(), "{provenance}");
        assert_eq!(provenance["source"], "generated", "other provenance stays");
    }

    #[test]
    fn dismissing_a_proposal_leaves_the_key_unchanged() {
        let w = world(wrong_answer);
        let attempt = start(&w, FeedbackMode::Study);
        answer_with(&w, &attempt, &w.old_key);
        ask_it(&w, &attempt).unwrap();
        w.db.with(|conn| dismiss(conn, &w.jobs, &attempt, 0))
            .unwrap();
        let error =
            w.db.transaction(|tx| apply(tx, &w.jobs, &attempt, 0, T0))
                .unwrap_err();
        assert!(error.to_string().contains("Peça de novo"));
        let key: String =
            w.db.with(|c| Ok(c.query_row("SELECT answer_id FROM questions", [], |r| r.get(0))?))
                .unwrap();
        assert_eq!(key, w.old_key);
        let changes: i64 =
            w.db.with(|c| Ok(c.query_row("SELECT COUNT(*) FROM key_changes", [], |r| r.get(0))?))
                .unwrap();
        assert_eq!(changes, 0);
    }

    #[test]
    fn a_correct_verdict_proposes_no_change() {
        assert_no_correction(RecheckVerdict::Correct);
    }

    #[test]
    fn an_ambiguous_verdict_proposes_no_change() {
        assert_no_correction(RecheckVerdict::Ambiguous);
    }

    fn assert_no_correction(verdict: RecheckVerdict) {
        let w = world(
            |_, _| json!({ "verdict": verdict, "correctedKey": "", "reasoning": "Está ok.", "sources": [] }),
        );
        let attempt = answered_attempt(&w);
        assert_eq!(
            ask_it(&w, &attempt).unwrap(),
            RecheckView {
                verdict,
                corrected_key: None,
                reasoning: "Está ok.".into(),
                sources: vec![],
            }
        );
        let error = confirm(&w, &attempt).unwrap_err();
        assert!(error.to_string().contains("não propõe trocar"));
    }

    #[test]
    fn a_wrong_verdict_needs_a_real_different_option() {
        for bad_key in ["", "nao-existe", "SAME"] {
            let w = world(|old, _| {
                let key = if bad_key == "SAME" { old } else { bad_key };
                json!({ "verdict": "wrong", "correctedKey": key, "reasoning": "x", "sources": [] })
            });
            let attempt = start(&w, FeedbackMode::Study);
            answer_with(&w, &attempt, &w.old_key);
            let error = ask_it(&w, &attempt).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("não indicou uma alternativa válida"),
                "{bad_key}"
            );
            assert!(w.jobs.take(&w.question_id).is_none(), "nothing is kept");
        }
    }

    #[test]
    fn an_answer_without_reasoning_is_refused() {
        let w = world(
            |_, _| json!({ "verdict": "correct", "correctedKey": "", "reasoning": "  ", "sources": [] }),
        );
        let attempt = start(&w, FeedbackMode::Study);
        answer_with(&w, &attempt, &w.old_key);
        assert!(ask_it(&w, &attempt).is_err());
    }

    #[test]
    fn sources_at_the_bank_limits_are_accepted() {
        let mut sources: Vec<_> = (1..50).map(|index| format!("Fonte {index}")).collect();
        sources.push("á".repeat(2_000));
        let w = world(|_, _| {
            json!({
                "verdict": "correct",
                "correctedKey": "",
                "reasoning": "Está ok.",
                "sources": sources
            })
        });
        let attempt = start(&w, FeedbackMode::Study);
        answer_with(&w, &attempt, &w.old_key);

        assert_eq!(ask_it(&w, &attempt).unwrap().sources, sources);
    }

    #[test]
    fn too_many_sources_are_refused() {
        assert_invalid_sources(vec!["Fonte".to_owned(); 51]);
    }

    #[test]
    fn a_source_over_the_length_limit_is_refused() {
        assert_invalid_sources(vec!["á".repeat(2_001)]);
    }

    fn assert_invalid_sources(sources: Vec<String>) {
        let w = world(|_, _| {
            json!({
                "verdict": "correct",
                "correctedKey": "",
                "reasoning": "Está ok.",
                "sources": sources
            })
        });
        let attempt = answered_attempt(&w);

        let error = ask_it(&w, &attempt).unwrap_err();

        assert!(error.to_string().contains("fontes inválidas"), "{error}");
        assert!(w.jobs.take(&w.question_id).is_none());
    }

    #[test]
    fn a_proposal_is_stale_if_the_key_changed_after_it_was_made() {
        let w = world(wrong_answer);
        let attempt = start(&w, FeedbackMode::Study);
        answer_with(&w, &attempt, &w.old_key);
        ask_it(&w, &attempt).unwrap();
        // Someone applied a different change to the same question in the meantime.
        w.db.with(|conn| {
            conn.execute(
                "UPDATE questions SET answer_id = ?1",
                params![w.other_option],
            )?;
            Ok(())
        })
        .unwrap();
        let error =
            w.db.transaction(|tx| apply(tx, &w.jobs, &attempt, 0, T0))
                .unwrap_err();
        assert!(error.to_string().contains("mudou desde a revisão"));
    }

    #[test]
    fn confirming_requires_a_proposal_the_backend_made() {
        let w = world(wrong_answer);
        let attempt = start(&w, FeedbackMode::Study);
        answer_with(&w, &attempt, &w.old_key);
        let error =
            w.db.transaction(|tx| apply(tx, &w.jobs, &attempt, 0, T0))
                .unwrap_err();
        assert!(error.to_string().contains("Peça de novo"));
    }
}

#[test]
fn the_recheck_schema_is_strict() {
    let schema = crate::ai::task::schema_of::<RecheckAnswer>();
    assert_eq!(schema["additionalProperties"], json!(false));
    assert!(!schema.to_string().contains("anyOf"));
}

#[test]
fn the_recheck_schema_matches_its_snapshot() {
    let schema = crate::ai::task::schema_of::<RecheckAnswer>();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/recheck.schema.json");
    let generated = serde_json::to_string_pretty(&schema).unwrap() + "\n";
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &generated).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap_or_default(),
        generated,
        "run `UPDATE_SNAPSHOTS=1 cargo test` and commit schemas/recheck.schema.json"
    );
}
