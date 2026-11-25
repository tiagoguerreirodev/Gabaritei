#![cfg_attr(not(unix), allow(dead_code, unused_imports))] // fake CLIs need `sh`

use super::*;
use crate::attempts::{FeedbackMode, SessionSource, SessionStart, answer as answer_question};
use crate::bank::import_questions;
use crate::bank::question::tests::multiple;
use crate::bank::store::load_question;
use rand::SeedableRng;
use serde_json::json;
use std::path::{Path, PathBuf};

const T0: &str = "2026-11-20T10:00:00.000Z";
const EXPLAINED_AT: &str = "2026-11-20T11:00:00.000Z";

struct World {
    dir: tempfile::TempDir,
    db: Database,
    secrets: Secrets,
    clis: ResolvedClis,
    attempt: AttemptId,
    key: String,
}

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

/// One question (with `explanation` and `refs`), an attempt in `mode`, the question answered.
#[cfg(unix)]
fn world(
    explanation: &str,
    mode: FeedbackMode,
    answered: bool,
    answer: serde_json::Value,
) -> World {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_in_memory().unwrap();
    let secrets = Secrets::memory();
    let (id, key) = db
        .transaction(|tx| {
            let mut input = multiple("q1", "Primeira", 0);
            input.explanation = explanation.into();
            input.refs = vec!["Lei 1".into()];
            let outcome = import_questions(tx, vec![input], T0)?;
            let id = outcome.ids_by_local_id["q1"].clone();
            tx.execute(
                "INSERT INTO collections (id, title, kind, created_at) VALUES ('c1', 'Caderno', ?1, ?2)",
                params![crate::bank::catalog::CollectionKind::Imported, T0],
            )?;
            tx.execute(
                "INSERT INTO collection_items (collection_id, position, question_id) VALUES ('c1', 0, ?1)",
                params![id],
            )?;
            let key = load_question(tx, &id)?.unwrap().answer_id;
            Ok((id, key))
        })
        .unwrap();
    let _ = id;
    std::fs::write(dir.path().join("answer.json"), answer.to_string()).unwrap();
    let cli = fake_codex(dir.path());
    let attempt = db
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
                &mut rand::rngs::StdRng::seed_from_u64(1),
            )
        })
        .unwrap()
        .id;
    if answered {
        db.transaction(|tx| answer_question(tx, &attempt, 0, Some(&key), T0))
            .unwrap();
    }
    World {
        dir,
        db,
        secrets,
        clis: ResolvedClis::new(None, Some(cli)),
        attempt,
        key,
    }
}

fn calls(world: &World) -> usize {
    std::fs::read_to_string(world.dir.path().join("calls"))
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

fn explain(world: &World) -> AppResult<AttemptView> {
    ask_explain(
        &world.db,
        &world.secrets,
        &world.clis,
        &world.attempt,
        0,
        &CancelToken::new(),
        || EXPLAINED_AT.into(),
    )
}

fn good() -> serde_json::Value {
    json!({ "explanation": "A alternativa certa é a B porque o art. 5º assim dispõe.", "refs": ["CF/88, art. 5º", "Lei 1"] })
}

#[cfg(unix)]
mod flow {
    use super::*;

    #[test]
    fn it_saves_a_missing_explanation() {
        let w = world("", FeedbackMode::Study, true, good());
        let view = explain(&w).unwrap();
        let reveal = view.questions[0].reveal.as_ref().unwrap();
        assert!(reveal.explanation.starts_with("A alternativa certa é a B"));
        let stored: (String, String) =
            w.db.with(|c| {
                Ok(
                    c.query_row("SELECT explanation, updated_at FROM questions", [], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })?,
                )
            })
            .unwrap();
        assert_eq!(stored, (reveal.explanation.clone(), EXPLAINED_AT.into()));
    }

    #[test]
    fn it_adds_distinct_references_to_the_existing_ones() {
        let w = world("", FeedbackMode::Study, true, good());
        let view = explain(&w).unwrap();
        assert_eq!(
            view.questions[0].reveal.as_ref().unwrap().refs,
            ["Lei 1", "CF/88, art. 5º"]
        );
    }

    #[test]
    fn the_prompt_names_the_correct_option_by_letter() {
        let w = world("", FeedbackMode::Study, true, good());
        explain(&w).unwrap();
        let sent = std::fs::read_to_string(w.dir.path().join("stdin.txt")).unwrap();
        // The key is named by letter, never by the internal hex id, and the AI is told to cite
        // the alternative's content because the order is shuffled in the exam.
        assert!(sent.contains("Gabarito: (A)") || sent.contains("Gabarito: (B)"));
        assert!(!sent.contains(&w.key));
        assert!(sent.contains("cite o conteúdo das alternativas"));
        assert!(sent.contains("NUNCA invente"));
    }

    #[test]
    fn sources_at_the_bank_limits_are_accepted() {
        let mut refs = vec!["Lei 1".to_owned(), "á".repeat(2_000)];
        refs.extend((2..50).map(|index| format!("Fonte {index}")));
        let w = world(
            "",
            FeedbackMode::Study,
            true,
            json!({ "explanation": "Texto.", "refs": refs }),
        );

        let view = explain(&w).unwrap();

        assert_eq!(view.questions[0].reveal.as_ref().unwrap().refs, refs);
    }

    #[test]
    fn too_many_sources_in_the_reply_are_refused_before_deduplicating() {
        let w = world(
            "",
            FeedbackMode::Study,
            true,
            json!({ "explanation": "Texto.", "refs": vec!["Lei 1"; 51] }),
        );

        let error = explain(&w).unwrap_err();

        assert!(error.to_string().contains("fontes demais"), "{error}");
    }

    #[test]
    fn too_many_sources_after_merging_are_refused() {
        let refs: Vec<_> = (1..=50).map(|index| format!("Fonte {index}")).collect();
        let w = world(
            "",
            FeedbackMode::Study,
            true,
            json!({ "explanation": "Texto.", "refs": refs }),
        );

        let error = explain(&w).unwrap_err();

        assert!(error.to_string().contains("fontes demais"), "{error}");
    }

    #[test]
    fn a_source_that_exceeds_the_length_limit_is_refused_before_trimming() {
        let reference = format!("{} ", "á".repeat(2_000));
        let w = world(
            "",
            FeedbackMode::Study,
            true,
            json!({ "explanation": "Texto.", "refs": [reference] }),
        );

        let error = explain(&w).unwrap_err();

        assert!(error.to_string().contains("fontes inválidas"), "{error}");
    }

    #[test]
    fn a_whitespace_only_explanation_counts_as_missing() {
        let w = world("   \n", FeedbackMode::Study, true, good());
        assert!(explain(&w).is_ok());
    }

    #[test]
    fn it_never_replaces_an_existing_explanation() {
        let w = world(
            "Comentário que já existe.",
            FeedbackMode::Study,
            true,
            good(),
        );
        let error = explain(&w).unwrap_err();
        assert!(error.to_string().contains("já tem explicação"));
        assert_eq!(calls(&w), 0, "no AI call for a question that has one");
    }

    #[test]
    fn it_is_refused_before_the_answer_is_revealed() {
        let mut w = world("", FeedbackMode::Exam, true, good());
        w.clis = ResolvedClis::new(None, None);
        let error = explain(&w).unwrap_err();
        assert!(error.to_string().contains("depois de ver o gabarito"));
        assert_eq!(calls(&w), 0);
    }

    #[test]
    fn a_blank_explanation_saves_nothing() {
        assert_explanation_rejected(json!({ "explanation": "  ", "refs": [] }));
    }

    #[test]
    fn a_blank_reference_saves_nothing() {
        assert_explanation_rejected(json!({ "explanation": "Texto.", "refs": ["  "] }));
    }

    #[test]
    fn an_answer_with_an_unknown_field_saves_nothing() {
        assert_explanation_rejected(json!({ "explanation": "Texto.", "extra": 1, "refs": [] }));
    }

    fn assert_explanation_rejected(answer: serde_json::Value) {
        let w = world("", FeedbackMode::Study, true, answer);
        assert!(explain(&w).is_err());
        let stored: String =
            w.db.with(|c| Ok(c.query_row("SELECT explanation FROM questions", [], |r| r.get(0))?))
                .unwrap();
        assert_eq!(stored, "");
    }
}

#[test]
fn the_explain_schema_is_strict() {
    let schema = crate::ai::task::schema_of::<ExplainAnswer>();
    assert_eq!(schema["additionalProperties"], json!(false));
}

#[test]
fn the_explain_schema_matches_its_snapshot() {
    let schema = crate::ai::task::schema_of::<ExplainAnswer>();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/explain.schema.json");
    let generated = serde_json::to_string_pretty(&schema).unwrap() + "\n";
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &generated).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap_or_default(),
        generated,
        "run `UPDATE_SNAPSHOTS=1 cargo test` and commit schemas/explain.schema.json"
    );
}
