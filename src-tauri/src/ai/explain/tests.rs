#![cfg_attr(not(unix), allow(dead_code, unused_imports))] // fake CLIs need `sh`

use super::*;
use crate::ai::settings::AiSettingsInput;
use crate::attempts::{FeedbackMode, SessionSource, SessionStart, answer as answer_question};
use crate::bank::import_questions;
use crate::bank::question::tests::multiple;
use serde_json::json;
use std::path::{Path, PathBuf};

const T0: &str = "2026-11-20T10:00:00.000Z";

struct World {
    dir: tempfile::TempDir,
    db: Database,
    secrets: Secrets,
    jobs: OnDemand,
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
    db.with(|conn| {
        let mut current = settings::load(conn, &secrets)?;
        current.cli_paths.codex = Some(cli.to_string_lossy().into_owned());
        settings::save(
            conn,
            AiSettingsInput {
                cli_paths: current.cli_paths,
                roles: current.roles,
                remnote: current.remnote,
            },
        )
    })
    .unwrap();
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
        jobs: OnDemand::default(),
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
    ask_explain(&world.db, &world.secrets, &world.jobs, &world.attempt, 0)
}

fn good() -> serde_json::Value {
    json!({ "explanation": "A alternativa certa é a B porque o art. 5º assim dispõe.", "refs": ["CF/88, art. 5º", "Lei 1"] })
}

#[cfg(unix)]
mod flow {
    use super::*;

    #[test]
    fn it_fills_in_a_missing_explanation_and_merges_the_references() {
        let w = world("", FeedbackMode::Study, true, good());
        let view = explain(&w).unwrap();
        let reveal = view.questions[0].reveal.as_ref().unwrap();
        assert!(reveal.explanation.starts_with("A alternativa certa é a B"));
        assert_eq!(reveal.refs, ["Lei 1", "CF/88, art. 5º"]);
        let stored: String =
            w.db.with(|c| Ok(c.query_row("SELECT explanation FROM questions", [], |r| r.get(0))?))
                .unwrap();
        assert_eq!(stored, reveal.explanation);
        let sent = std::fs::read_to_string(w.dir.path().join("stdin.txt")).unwrap();
        // The key is named by letter, never by the internal hex id, and the AI is told to cite
        // the alternative's content because the order is shuffled in the exam.
        assert!(sent.contains("Gabarito: (A)") || sent.contains("Gabarito: (B)"));
        assert!(!sent.contains(&w.key));
        assert!(sent.contains("cite o conteúdo das alternativas"));
        assert!(sent.contains("NUNCA invente"));
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
        let w = world("", FeedbackMode::Exam, true, good());
        let error = explain(&w).unwrap_err();
        assert!(error.to_string().contains("depois de ver o gabarito"));
        assert_eq!(calls(&w), 0);
    }

    #[test]
    fn bad_answers_save_nothing() {
        for bad in [
            json!({ "explanation": "  ", "refs": [] }),
            json!({ "explanation": "Texto.", "refs": ["  "] }),
            json!({ "explanation": "Texto.", "extra": 1, "refs": [] }),
        ] {
            let w = world("", FeedbackMode::Study, true, bad);
            assert!(explain(&w).is_err());
            let stored: String = w
                .db
                .with(|c| Ok(c.query_row("SELECT explanation FROM questions", [], |r| r.get(0))?))
                .unwrap();
            assert_eq!(stored, "");
        }
    }
}

#[test]
fn the_explain_schema_is_strict_and_matches_its_snapshot() {
    let schema = crate::ai::blind_check::schema_of::<ExplainAnswer>();
    assert_eq!(schema["additionalProperties"], json!(false));
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
