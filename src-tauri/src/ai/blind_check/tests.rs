#![cfg_attr(not(unix), allow(dead_code, unused_imports))] // fake CLIs need `sh`

use super::*;
use crate::ai::agent::{Agent, Effort};
use crate::ai::draft::{DraftRequest, GenerationInfo};
use crate::ai::generate::GenerateSources;
use crate::ai::settings::AiSettingsInput;
use crate::bank::Package;
use crate::bank::question::tests::topic;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const SECRET_EXPLANATION: &str = "EXPLICACAO-SECRETA-DO-GABARITO";
const SECRET_REF: &str = "REFERENCIA-SECRETA";

fn question(id: &str, key: &str) -> Value {
    json!({
        "id": id, "type": "multiple", "stem": format!("Enunciado da questão {id}?"),
        "options": [
            {"id": "a", "text": "Alternativa A"}, {"id": "b", "text": "Alternativa B"},
            {"id": "c", "text": "Alternativa C"}, {"id": "d", "text": "Alternativa D"},
            {"id": "e", "text": "Alternativa E"}
        ],
        "answerId": key, "explanation": SECRET_EXPLANATION, "refs": [SECRET_REF]
    })
}

fn package(questions: Vec<Value>) -> Package {
    Package::parse(&json!({ "questions": questions, "notes": [] }).to_string()).unwrap()
}

fn answer(id: &str, choice: &str) -> Value {
    json!({ "questionId": id, "choice": choice, "confidence": "high", "reason": "Porque sim." })
}

fn answers(items: Vec<Value>) -> Value {
    json!({ "verdicts": items })
}

fn resolution(id: &str, revised: Option<Value>) -> Value {
    json!({
        "questionId": id,
        "action": if revised.is_some() { "revise" } else { "defend" },
        "revised": revised.into_iter().collect::<Vec<_>>(),
        "rationale": "Racional."
    })
}

fn resolutions(items: Vec<Value>) -> Value {
    json!({ "resolutions": items })
}

/// A fake `codex exec` that answers call N with `answer-N.json` and records each prompt in
/// `stdin-N.txt`.
#[cfg(unix)]
fn fake_cli(dir: &Path, replies: &[Value]) -> PathBuf {
    for (index, reply) in replies.iter().enumerate() {
        std::fs::write(
            dir.join(format!("answer-{}.json", index + 1)),
            reply.to_string(),
        )
        .unwrap();
    }
    crate::ai::runner::tests::write_script(
        dir,
        "codex",
        &format!(
            r#"dir='{}'
n=$(cat "$dir/counter" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "$dir/counter"
out=""
while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift;; esac; shift; done
cat > "$dir/stdin-$n.txt"
ls -A > "$dir/files-$n.txt"
cp "$dir/answer-$n.json" "$out""#,
            dir.display()
        ),
    )
}

fn calls(dir: &Path) -> usize {
    std::fs::read_to_string(dir.join("counter"))
        .map(|text| text.trim().parse().unwrap())
        .unwrap_or(0)
}

fn files(dir: &Path, n: usize) -> Vec<String> {
    std::fs::read_to_string(dir.join(format!("files-{n}.txt")))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn prompt(dir: &Path, n: usize) -> String {
    std::fs::read_to_string(dir.join(format!("stdin-{n}.txt"))).unwrap()
}

struct Setup {
    dir: tempfile::TempDir,
    db: Database,
    secrets: Secrets,
    draft_id: String,
}

fn files_dir(s: &Setup) -> PathBuf {
    s.dir.path().join("draft-files")
}

#[cfg(unix)]
fn setup(package: Package, replies: &[Value]) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let cli = fake_cli(dir.path(), replies);
    let db = Database::open_in_memory().unwrap();
    let secrets = Secrets::memory();
    db.with(|conn| {
        let mut current = settings::load(conn, &secrets)?;
        current.cli_paths.codex = Some(cli.to_string_lossy().into_owned());
        let roles = current.roles.clone();
        settings::save(
            conn,
            AiSettingsInput {
                cli_paths: current.cli_paths,
                roles,
                remnote: current.remnote,
            },
        )
    })
    .unwrap();
    let roles = crate::ai::roles::RoleSettings::default();
    let draft = Draft {
        id: "draft-1".into(),
        created_at: "2026-11-20T00:00:00.000Z".into(),
        status: DraftStatus::Review,
        request: DraftRequest {
            request: GenerateRequest {
                topic: topic(),
                count: u32::try_from(package.questions.len()).unwrap(),
                board: None,
                format: None,
                instructions: None,
                sources: GenerateSources::default(),
                generator: None,
                checker: None,
            },
            info: GenerationInfo {
                agent: Agent::Codex,
                model: roles.generate.model.clone(),
                effort: Effort::Xhigh,
                started_at: "2026-11-20T00:00:00.000Z".into(),
                duration_ms: 1,
                cost_usd: None,
            },
            generator: roles.generate,
            checker: roles.blind_check,
        },
        package,
        blind_check: None,
    };
    db.with(|conn| draft::insert(conn, &draft)).unwrap();
    Setup {
        dir,
        db,
        secrets,
        draft_id: draft.id,
    }
}

fn run_check(setup: &Setup) -> AppResult<BlindCheck> {
    run(
        &setup.db,
        &setup.secrets,
        &setup.draft_id,
        &[],
        &CancelToken::new(),
        &mut |_| {},
    )
}

fn states(check: &BlindCheck) -> Vec<(&str, VerdictState)> {
    check
        .verdicts
        .iter()
        .map(|v| (v.question_id.as_str(), v.state))
        .collect()
}

#[cfg(unix)]
mod flow {
    use super::*;

    #[test]
    fn a_checker_that_agrees_everywhere_needs_no_dispute() {
        let s = setup(
            package(vec![question("q1", "c"), question("q2", "a")]),
            &[answers(vec![answer("q1", "c"), answer("q2", "a")])],
        );
        let check = run_check(&s).unwrap();
        assert_eq!(
            states(&check),
            [("q1", VerdictState::Agreed), ("q2", VerdictState::Agreed)]
        );
        assert_eq!(
            calls(s.dir.path()),
            1,
            "no generator call without a disagreement"
        );
        let stored = s.db.with(|conn| draft::load(conn, &s.draft_id)).unwrap();
        assert_eq!(super::super::stored(&stored).unwrap().unwrap(), check);
    }

    #[test]
    fn a_disagreement_fixed_by_the_generator_and_accepted_on_recheck() {
        let revised = question("q2", "b");
        let s = setup(
            package(vec![question("q1", "c"), question("q2", "a")]),
            &[
                answers(vec![answer("q1", "c"), answer("q2", "b")]),
                resolutions(vec![resolution("q2", Some(revised.clone()))]),
                answers(vec![answer("q2", "b")]),
            ],
        );
        let check = run_check(&s).unwrap();
        assert_eq!(
            states(&check),
            [("q1", VerdictState::Agreed), ("q2", VerdictState::Fixed)]
        );
        assert!(check.verdicts[1].objected);
        assert_eq!(calls(s.dir.path()), 3);
        // Only the revised question goes back to the checker.
        let third = prompt(s.dir.path(), 3);
        assert!(third.contains("Questão q2") && !third.contains("Questão q1"));
        // The revision replaced the question in the draft.
        let stored = s.db.with(|conn| draft::load(conn, &s.draft_id)).unwrap();
        assert_eq!(stored.package.questions[1].answer_id, "b");
        assert_eq!(stored.package.questions[0].answer_id, "c");
    }

    #[test]
    fn a_revision_the_checker_still_rejects_stays_disputed() {
        let s = setup(
            package(vec![question("q1", "c")]),
            &[
                answers(vec![answer("q1", "a")]),
                resolutions(vec![resolution("q1", Some(question("q1", "b")))]),
                answers(vec![answer("q1", "d")]),
            ],
        );
        let check = run_check(&s).unwrap();
        assert_eq!(states(&check), [("q1", VerdictState::Disputed)]);
        assert_eq!(check.verdicts[0].final_choice, "d");
        assert_eq!(check.verdicts[0].first_choice, "a");
    }

    #[test]
    fn a_defended_question_is_disputed_without_a_second_check() {
        let s = setup(
            package(vec![question("q1", "c")]),
            &[
                answers(vec![answer("q1", "a")]),
                resolutions(vec![resolution("q1", None)]),
            ],
        );
        let check = run_check(&s).unwrap();
        assert_eq!(states(&check), [("q1", VerdictState::Disputed)]);
        assert_eq!(calls(s.dir.path()), 2);
        let stored = s.db.with(|conn| draft::load(conn, &s.draft_id)).unwrap();
        assert_eq!(
            stored.package.questions[0].answer_id, "c",
            "a defended question is unchanged"
        );
    }

    #[test]
    fn a_revision_for_the_wrong_question_counts_as_a_defence() {
        let s = setup(
            package(vec![question("q1", "c")]),
            &[
                answers(vec![answer("q1", "a")]),
                resolutions(vec![resolution("q1", Some(question("q9", "a")))]),
            ],
        );
        let check = run_check(&s).unwrap();
        assert_eq!(states(&check), [("q1", VerdictState::Disputed)]);
        assert_eq!(calls(s.dir.path()), 2);
    }

    #[test]
    fn the_checkers_prompt_has_no_answer_key_explanation_or_reference() {
        let s = setup(
            package(vec![question("q1", "c"), question("q2", "a")]),
            &[answers(vec![answer("q1", "c"), answer("q2", "a")])],
        );
        run_check(&s).unwrap();
        let sent = prompt(s.dir.path(), 1);
        assert!(sent.contains("Enunciado da questão q1?") && sent.contains("(e) Alternativa E"));
        for secret in [
            SECRET_EXPLANATION,
            SECRET_REF,
            "answerId",
            "explanation",
            "refs",
        ] {
            assert!(
                !sent.contains(secret),
                "the blind checker must not see {secret:?}"
            );
        }
        assert!(!sent.contains("Seu gabarito"));
        // No input file either: the run folder holds only the schema.
        assert_eq!(files(s.dir.path(), 1), ["schema.json"]);
    }

    #[test]
    fn the_dispute_round_gets_the_attachments_the_generator_wrote_from() {
        let s = setup(
            package(vec![question("q1", "c")]),
            &[
                answers(vec![answer("q1", "a")]),
                resolutions(vec![resolution("q1", None)]),
            ],
        );
        let file = s.dir.path().join("Lei.md");
        std::fs::write(&file, "texto").unwrap();
        run(
            &s.db,
            &s.secrets,
            &s.draft_id,
            std::slice::from_ref(&file),
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap();
        // The blind checker never gets attachments; the generator answering the objection does.
        assert_eq!(files(s.dir.path(), 1), ["schema.json"]);
        assert!(files(s.dir.path(), 2).contains(&"anexos".to_owned()));
    }

    #[test]
    fn late_writes_never_touch_a_draft_that_was_imported_or_discarded() {
        let s = setup(package(vec![question("q1", "c")]), &[]);
        s.db.with(|conn| draft::set_status(conn, &s.draft_id, DraftStatus::Imported))
            .unwrap();
        let verdicts = pending_record();
        s.db.with(|conn| draft::set_blind_check(conn, &s.draft_id, &verdicts))
            .unwrap();
        s.db.with(|conn| {
            draft::set_package(conn, &s.draft_id, &package(vec![question("q2", "a")]))
        })
        .unwrap();
        let stored = s.db.with(|conn| draft::load(conn, &s.draft_id)).unwrap();
        assert_eq!(stored.status, DraftStatus::Imported);
        assert!(
            stored.blind_check.is_none(),
            "the verdicts were not written"
        );
        assert_eq!(stored.package.questions[0].id, "q1");
        // A second move out of review is refused instead of silently overwriting.
        assert!(
            s.db.with(|conn| draft::set_status(conn, &s.draft_id, DraftStatus::Discarded))
                .is_err()
        );
    }

    #[test]
    fn a_retry_is_refused_while_a_check_is_running_and_after_a_restart_it_recovers() {
        let s = setup(
            package(vec![question("q1", "c")]),
            &[answers(vec![answer("q1", "c")])],
        );
        s.db.with(|conn| draft::set_blind_check(conn, &s.draft_id, &pending_record()))
            .unwrap();
        let error = recheck_draft(
            &s.db,
            &s.secrets,
            &files_dir(&s),
            &s.draft_id,
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(error.to_string().contains("já está em andamento"));
        assert_eq!(calls(s.dir.path()), 0);

        // After a restart nothing can still be running: the check is marked interrupted.
        s.db.with(|conn| recover_interrupted(conn)).unwrap();
        let stored = s.db.with(|conn| draft::load(conn, &s.draft_id)).unwrap();
        let check = super::stored(&stored).unwrap().unwrap();
        assert!(!check.pending);
        assert!(check.failed.unwrap().contains("interrompida"));
        recheck_draft(
            &s.db,
            &s.secrets,
            &files_dir(&s),
            &s.draft_id,
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap();
        let stored = s.db.with(|conn| draft::load(conn, &s.draft_id)).unwrap();
        assert_eq!(
            states(&super::stored(&stored).unwrap().unwrap()),
            [("q1", VerdictState::Agreed)]
        );
    }

    #[test]
    fn answers_that_do_not_match_the_questions_are_refused() {
        for bad in [
            answers(vec![answer("q1", "c")]),                    // q2 missing
            answers(vec![answer("q1", "z"), answer("q2", "a")]), // no such option
            answers(vec![answer("q1", "c"), answer("q1", "c")]), // answered twice
            answers(vec![answer("q1", "c"), answer("q7", "a")]), // unknown question
        ] {
            let s = setup(
                package(vec![question("q1", "c"), question("q2", "a")]),
                &[bad],
            );
            assert!(run_check(&s).is_err());
            let stored = s.db.with(|conn| draft::load(conn, &s.draft_id)).unwrap();
            assert!(
                stored.blind_check.is_none(),
                "nothing is stored on a failed check"
            );
        }
    }
}

#[cfg(unix)]
mod pipeline {
    use super::*;

    fn generated(questions: Vec<Value>) -> Value {
        json!({ "questions": questions, "notes": [] })
    }

    fn request(count: u32) -> GenerateRequest {
        GenerateRequest {
            topic: topic(),
            count,
            board: None,
            format: None,
            instructions: None,
            sources: GenerateSources::default(),
            generator: None,
            checker: None,
        }
    }

    fn setup_pipeline(replies: &[Value]) -> Setup {
        // The pipeline creates its own draft, so start from an empty package here.
        let mut s = setup(package(vec![]), replies);
        s.db.with(|conn| {
            conn.execute("DELETE FROM drafts", [])?;
            Ok(())
        })
        .unwrap();
        s.draft_id.clear();
        s
    }

    fn only_draft(db: &Database) -> Draft {
        let id: String = db
            .with(|conn| Ok(conn.query_row("SELECT id FROM drafts", [], |r| r.get(0))?))
            .unwrap();
        db.with(|conn| draft::load(conn, &id)).unwrap()
    }

    #[test]
    fn generation_then_a_blind_check_saves_one_draft_with_verdicts() {
        let s = setup_pipeline(&[
            generated(vec![question("q1", "c")]),
            answers(vec![answer("q1", "c")]),
        ]);
        let mut labels = Vec::new();
        let id = generate_and_check(
            &s.db,
            &s.secrets,
            &files_dir(&s),
            request(1),
            &CancelToken::new(),
            &mut |p| {
                labels.push(p.label);
            },
        )
        .unwrap();
        let draft = s.db.with(|conn| draft::load(conn, &id)).unwrap();
        let check = stored(&draft).unwrap().unwrap();
        assert_eq!(states(&check), [("q1", VerdictState::Agreed)]);
        assert!(labels.iter().any(|l| l.contains("Gerando")));
        assert!(labels.iter().any(|l| l.contains("Conferindo")));
    }

    /// The stages a run announces, in order, without repeats.
    fn stages_of(events: &[Progress]) -> Vec<GenerationStage> {
        let mut stages: Vec<GenerationStage> = events.iter().filter_map(|p| p.stage).collect();
        stages.dedup();
        stages
    }

    #[test]
    fn a_generation_announces_its_stages_in_order() {
        let s = setup_pipeline(&[
            generated(vec![question("q1", "c")]),
            answers(vec![answer("q1", "a")]),
            resolutions(vec![resolution("q1", None)]),
        ]);
        let mut events = Vec::new();
        generate_and_check(
            &s.db,
            &s.secrets,
            &files_dir(&s),
            request(1),
            &CancelToken::new(),
            &mut |p| events.push(p),
        )
        .unwrap();
        assert_eq!(
            stages_of(&events),
            [
                GenerationStage::Generating,
                GenerationStage::Checking,
                GenerationStage::Resolving
            ]
        );
    }

    #[test]
    fn a_recheck_never_announces_generating_and_the_missing_files_notice_keeps_the_stage() {
        let s = setup_pipeline(&[
            generated(vec![question("q1", "c")]),
            answers(vec![]),
            answers(vec![answer("q1", "a")]),
            resolutions(vec![resolution("q1", None)]),
        ]);
        let original = s.dir.path().join("Lei.md");
        std::fs::write(&original, "texto").unwrap();
        let mut job = request(1);
        job.sources.attachments = vec![original.to_string_lossy().into_owned()];
        let id = generate_and_check(
            &s.db,
            &s.secrets,
            &files_dir(&s),
            job,
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap();
        draft_files::remove(&files_dir(&s), &id);
        let mut events = Vec::new();
        recheck_draft(
            &s.db,
            &s.secrets,
            &files_dir(&s),
            &id,
            &CancelToken::new(),
            &mut |p| events.push(p),
        )
        .unwrap();
        assert_eq!(
            stages_of(&events),
            [GenerationStage::Checking, GenerationStage::Resolving]
        );
        let notice = events
            .iter()
            .find(|p| p.label.contains("anexos originais"))
            .unwrap();
        assert_eq!(notice.stage, None, "the notice does not change the stage");
    }

    #[test]
    fn a_new_draft_is_marked_as_checking_until_the_check_ends() {
        // Only the generator answers; the check then fails, which also ends "pending".
        let s = setup_pipeline(&[generated(vec![question("q1", "c")])]);
        let draft = crate::ai::generate::generate_draft(
            &s.db,
            &s.secrets,
            request(1),
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap();
        let stored = s.db.with(|conn| draft::load(conn, &draft.id)).unwrap();
        assert!(super::stored(&stored).unwrap().unwrap().pending);
    }

    #[test]
    fn a_failed_check_keeps_the_draft_and_says_so() {
        let s = setup_pipeline(&[
            generated(vec![question("q1", "c")]),
            answers(vec![]), // the checker answered nothing
        ]);
        let id = generate_and_check(
            &s.db,
            &s.secrets,
            &files_dir(&s),
            request(1),
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap();
        let draft = only_draft(&s.db);
        assert_eq!(draft.id, id);
        assert_eq!(draft.status, DraftStatus::Review);
        let check = stored(&draft).unwrap().unwrap();
        assert!(check.verdicts.is_empty());
        assert!(check.failed.unwrap().contains("não respondeu todas"));
    }

    #[test]
    fn a_failed_check_can_be_run_again() {
        let s = setup_pipeline(&[
            generated(vec![question("q1", "c")]),
            answers(vec![]),
            answers(vec![answer("q1", "c")]),
        ]);
        let id = generate_and_check(
            &s.db,
            &s.secrets,
            &files_dir(&s),
            request(1),
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap();
        recheck_draft(
            &s.db,
            &s.secrets,
            &files_dir(&s),
            &id,
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap();
        let check = stored(&only_draft(&s.db)).unwrap().unwrap();
        assert!(check.failed.is_none());
        assert_eq!(states(&check), [("q1", VerdictState::Agreed)]);
    }

    #[test]
    fn a_retry_gives_the_dispute_round_the_attachments_kept_for_the_draft() {
        let s = setup_pipeline(&[
            generated(vec![question("q1", "c")]),
            answers(vec![]),
            answers(vec![answer("q1", "a")]),
            resolutions(vec![resolution("q1", None)]),
        ]);
        let original = s.dir.path().join("Lei.md");
        std::fs::write(&original, "texto").unwrap();
        let mut job = request(1);
        job.sources.attachments = vec![original.to_string_lossy().into_owned()];
        let id = generate_and_check(
            &s.db,
            &s.secrets,
            &files_dir(&s),
            job,
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(draft_files::load(&files_dir(&s), &id).unwrap().len(), 1);
        // The user may move or delete the file; the retry uses the kept copy.
        std::fs::remove_file(&original).unwrap();
        let mut progress = Vec::new();
        recheck_draft(
            &s.db,
            &s.secrets,
            &files_dir(&s),
            &id,
            &CancelToken::new(),
            &mut |p| progress.push(p.label),
        )
        .unwrap();
        assert!(files(s.dir.path(), 4).contains(&"anexos".to_owned()));
        assert!(
            progress
                .iter()
                .all(|label| !label.contains("anexos originais")),
            "no warning when the copies exist"
        );
        assert!(
            stored(&only_draft(&s.db))
                .unwrap()
                .unwrap()
                .failed
                .is_none()
        );
    }

    #[test]
    fn a_retry_without_kept_copies_says_so() {
        let s = setup_pipeline(&[
            generated(vec![question("q1", "c")]),
            answers(vec![]),
            answers(vec![answer("q1", "c")]),
        ]);
        let original = s.dir.path().join("Lei.md");
        std::fs::write(&original, "texto").unwrap();
        let mut job = request(1);
        job.sources.attachments = vec![original.to_string_lossy().into_owned()];
        let id = generate_and_check(
            &s.db,
            &s.secrets,
            &files_dir(&s),
            job,
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap();
        draft_files::remove(&files_dir(&s), &id);
        let mut progress = Vec::new();
        recheck_draft(
            &s.db,
            &s.secrets,
            &files_dir(&s),
            &id,
            &CancelToken::new(),
            &mut |p| progress.push(p.label),
        )
        .unwrap();
        assert!(progress.iter().any(|l| l.contains("anexos originais")));
    }

    #[test]
    fn kept_copies_hold_the_original_file_names_and_content() {
        let s = setup_pipeline(&[
            generated(vec![question("q1", "c")]),
            answers(vec![answer("q1", "c")]),
        ]);
        let original = s.dir.path().join("Lei.md");
        std::fs::write(&original, "texto").unwrap();
        let mut job = request(1);
        job.sources.attachments = vec![original.to_string_lossy().into_owned()];
        let id = generate_and_check(
            &s.db,
            &s.secrets,
            &files_dir(&s),
            job,
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap();
        let kept = draft_files::load(&files_dir(&s), &id).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].file_name().unwrap(), "Lei.md");
        assert_eq!(std::fs::read_to_string(&kept[0]).unwrap(), "texto");
    }

    #[test]
    fn cancelling_during_the_check_discards_the_draft() {
        let dir = tempfile::tempdir().unwrap();
        // The first call (generation) answers; the second (the check) hangs.
        std::fs::write(
            dir.path().join("answer-1.json"),
            generated(vec![question("q1", "c")]).to_string(),
        )
        .unwrap();
        let cli = crate::ai::runner::tests::write_script(
            dir.path(),
            "codex",
            &format!(
                r#"dir='{}'
n=$(cat "$dir/counter" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "$dir/counter"
out=""
while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift;; esac; shift; done
cat > /dev/null
if [ "$n" = 1 ]; then cp "$dir/answer-1.json" "$out"; else sleep 60; fi"#,
                dir.path().display()
            ),
        );
        let s = setup_pipeline(&[]);
        s.db.with(|conn| {
            let mut current = settings::load(conn, &s.secrets)?;
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
        let token = CancelToken::new();
        let canceller = token.clone();
        let counter = dir.path().join("counter");
        std::thread::spawn(move || {
            // Cancel once the second call (the check) has started, not after a fixed time:
            // under load the generation can take longer than any guess.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            while std::time::Instant::now() < deadline {
                if std::fs::read_to_string(&counter).is_ok_and(|n| n.trim() == "2") {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            // Let the check call get going before cancelling it.
            std::thread::sleep(std::time::Duration::from_millis(200));
            canceller.cancel();
        });
        let original = s.dir.path().join("Lei.md");
        std::fs::write(&original, "texto").unwrap();
        let mut job = request(1);
        job.sources.attachments = vec![original.to_string_lossy().into_owned()];
        let error = generate_and_check(&s.db, &s.secrets, &files_dir(&s), job, &token, &mut |_| {})
            .unwrap_err();
        assert!(error.to_string().contains("cancelada"));
        let draft = only_draft(&s.db);
        assert_eq!(draft.status, DraftStatus::Discarded);
        assert!(
            draft_files::load(&files_dir(&s), &draft.id)
                .unwrap()
                .is_empty(),
            "a cancelled run keeps no copies"
        );
    }
}

mod schemas {
    use super::*;

    fn snapshot(name: &str, schema: &Value) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("schemas")
            .join(name);
        let generated = serde_json::to_string_pretty(schema).unwrap() + "\n";
        if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
            std::fs::write(&path, &generated).unwrap();
        }
        let stored = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            stored, generated,
            "schemas/{name} is out of date. If the change is intended, run \
             `UPDATE_SNAPSHOTS=1 cargo test` and commit the file."
        );
    }

    #[test]
    fn the_checker_and_dispute_schemas_match_their_snapshots() {
        snapshot("checker.schema.json", &schema_of::<CheckerAnswers>());
        snapshot("dispute.schema.json", &schema_of::<DisputeResolutions>());
    }

    #[test]
    fn every_property_is_required_so_strict_structured_output_accepts_it() {
        for schema in [
            schema_of::<CheckerAnswers>(),
            schema_of::<DisputeResolutions>(),
        ] {
            let text = schema.to_string();
            assert!(!text.contains("\"anyOf\""), "no optional fields: {text}");
            for def in schema["$defs"].as_object().unwrap().values() {
                if let Some(properties) = def["properties"].as_object() {
                    let required: Vec<&str> = def["required"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_str().unwrap())
                        .collect();
                    for key in properties.keys() {
                        assert!(required.contains(&key.as_str()), "{key} must be required");
                    }
                }
            }
        }
    }

    #[test]
    fn the_checker_view_cannot_carry_a_key() {
        let question: PackageQuestion = serde_json::from_value(question("q1", "c")).unwrap();
        let view = CheckerView::from(&question);
        let shown = format!("{view:?}");
        for secret in [SECRET_EXPLANATION, SECRET_REF] {
            assert!(!shown.contains(secret));
        }
    }
}
