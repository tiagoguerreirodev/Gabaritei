use super::*;
use crate::ai::blind_check::{self, BlindCheck, Confidence, Verdict, pending_record};
use crate::ai::draft::{DraftRequest, GenerationInfo};
use crate::ai::generate::{GenerateRequest, GenerateSources};
use crate::ai::roles::RoleSettings;
use crate::bank::Package;
use crate::bank::question::tests::topic;
use crate::db::Database;
use serde_json::{Value, json};

const NOW: &str = "2026-11-20T12:00:00.000Z";

fn question(id: &str, key: &str) -> Value {
    json!({
        "id": id, "type": "multiple", "stem": format!("ENUNCIADO-{id} sobre o tema?"),
        "options": [
            {"id": "a", "text": format!("OPCAO-{id}-A")}, {"id": "b", "text": format!("OPCAO-{id}-B")},
            {"id": "c", "text": format!("OPCAO-{id}-C")}, {"id": "d", "text": format!("OPCAO-{id}-D")},
            {"id": "e", "text": format!("OPCAO-{id}-E")}
        ],
        "answerId": key, "explanation": format!("EXPLICACAO-{id}"), "refs": [format!("REFERENCIA-{id}")]
    })
}

fn verdict(id: &str, state: VerdictState) -> Verdict {
    Verdict {
        question_id: id.into(),
        state,
        first_choice: "a".into(),
        final_choice: "a".into(),
        confidence: Confidence::High,
        reason: format!("RAZAO-SECRETA-{id}"),
        objected: state != VerdictState::Agreed,
    }
}

fn draft_of(id: &str, questions: Vec<Value>, verdicts: Option<Vec<Verdict>>) -> Draft {
    let roles = RoleSettings::default();
    Draft {
        id: id.into(),
        created_at: "2026-11-19T21:00:00.000Z".into(),
        status: DraftStatus::Review,
        request: DraftRequest {
            request: GenerateRequest {
                topic: topic(),
                count: u32::try_from(questions.len()).unwrap(),
                board: Some("Cebraspe".into()),
                contest: None,
                position: None,
                format: None,
                instructions: None,
                sources: GenerateSources::default(),
                generator: None,
                checker: None,
            },
            info: GenerationInfo {
                agent: Agent::Codex,
                model: "gpt-6.2-sol".into(),
                effort: Effort::Xhigh,
                started_at: NOW.into(),
                duration_ms: 4200,
                cost_usd: None,
            },
            generator: roles.generate,
            checker: roles.blind_check,
        },
        package: Package::parse(&json!({ "questions": questions, "notes": [] }).to_string())
            .unwrap(),
        blind_check: verdicts.map(|verdicts| BlindCheck::Done {
            verdicts,
            info: None,
        }),
    }
}

fn insert(db: &Database, draft: &Draft) {
    db.with(|conn| draft::insert(conn, draft)).unwrap();
}

/// q1 agreed, q2 disputed, q3 fixed.
fn mixed(db: &Database) -> Draft {
    let draft = draft_of(
        "d1",
        vec![
            question("q1", "a"),
            question("q2", "b"),
            question("q3", "c"),
        ],
        Some(vec![
            verdict("q1", VerdictState::Agreed),
            verdict("q2", VerdictState::Disputed),
            verdict("q3", VerdictState::Fixed),
        ]),
    );
    insert(db, &draft);
    draft
}

fn summary_of(db: &Database, id: &str) -> DraftSummary {
    db.with(|conn| summary(conn, &DraftId::from(id))).unwrap()
}

fn import_with(db: &Database, id: &str, mode: ImportMode) -> AppResult<ImportResult> {
    let files = tempfile::tempdir().unwrap();
    import(db, files.path(), &DraftId::from(id), mode, NOW)
}

fn waiting_items(conn: &Connection) -> AppResult<Vec<DraftListItem>> {
    waiting(conn)
}

fn count(db: &Database, sql: &str) -> i64 {
    db.with(|conn| Ok(conn.query_row(sql, [], |row| row.get(0))?))
        .unwrap()
}

fn discard_with(db: &Database, id: &DraftId) -> AppResult<()> {
    let files = tempfile::tempdir().unwrap();
    discard(db, files.path(), id)
}

#[test]
fn a_question_the_checker_agreed_with_has_no_content_and_nothing_secret_leaks() {
    let db = Database::open_in_memory().unwrap();
    mixed(&db);
    let summary = summary_of(&db, "d1");
    assert_eq!(summary.count, 3);
    assert_eq!(
        (
            summary.agreed,
            summary.fixed,
            summary.disputed,
            summary.unchecked
        ),
        (1, 1, 1, 0)
    );
    assert_eq!((summary.multiple, summary.boolean), (3, 0));
    assert_eq!(
        summary
            .lines
            .iter()
            .map(|l| (l.position, l.state, l.content.is_some()))
            .collect::<Vec<_>>(),
        [
            (1, LineState::Agreed, false),
            (2, LineState::Disputed, true),
            (3, LineState::Fixed, false)
        ]
    );
    assert_eq!(summary.board.as_deref(), Some("Cebraspe"));

    // The disputed question shows its stem and options.
    let flagged = summary.lines[1].content.as_ref().unwrap();
    assert_eq!(flagged.stem, "ENUNCIADO-q2 sobre o tema?");
    assert_eq!(flagged.options.len(), 5);
    assert_eq!(flagged.options[0].text, "OPCAO-q2-A");

    // Never the key, explanation, references or the checker's reason, and nothing at all
    // from the questions that were not flagged.
    let json = serde_json::to_string(&summary).unwrap();
    for secret in [
        "ENUNCIADO-q1",
        "ENUNCIADO-q3",
        "OPCAO-q1",
        "OPCAO-q3",
        "EXPLICACAO",
        "REFERENCIA",
        "RAZAO-SECRETA",
        "answerId",
        "explanation",
        "refs",
    ] {
        assert!(
            !json.contains(secret),
            "the preview must not contain {secret:?}: {json}"
        );
    }
}

#[test]
fn a_question_with_a_validation_problem_is_flagged_even_if_the_checker_agreed() {
    let db = Database::open_in_memory().unwrap();
    let mut no_key = question("q2", "b");
    no_key["answerId"] = json!("z");
    insert(
        &db,
        &draft_of(
            "d1",
            vec![question("q1", "a"), no_key],
            Some(vec![
                verdict("q1", VerdictState::Agreed),
                verdict("q2", VerdictState::Agreed),
            ]),
        ),
    );
    let summary = summary_of(&db, "d1");
    assert!(summary.lines[0].content.is_none());
    assert!(summary.lines[1].content.is_some());
    let json = serde_json::to_string(&summary).unwrap();
    assert!(!json.contains("EXPLICACAO"), "{json}");
}

#[test]
fn an_unchecked_question_without_problems_shows_no_content() {
    let db = Database::open_in_memory().unwrap();
    insert(&db, &draft_of("d1", vec![question("q1", "a")], None));
    assert!(summary_of(&db, "d1").lines[0].content.is_none());
}

#[test]
fn problems_name_the_question_by_position_and_quote_nothing() {
    let db = Database::open_in_memory().unwrap();
    let mut repeated_option = question("q2", "b");
    repeated_option["options"][3]["text"] = repeated_option["options"][2]["text"].clone();
    let mut no_key = question("q3", "c");
    no_key["answerId"] = json!("z");
    let draft = draft_of(
        "d1",
        vec![question("q1", "a"), repeated_option, no_key],
        Some(vec![
            verdict("q1", VerdictState::Agreed),
            verdict("q2", VerdictState::Agreed),
            verdict("q3", VerdictState::Agreed),
        ]),
    );
    insert(&db, &draft);
    let summary = summary_of(&db, "d1");
    assert!(summary.problems.iter().any(|p| p.position == Some(2)));
    assert!(summary.problems.iter().any(|p| p.position == Some(3)));
    assert!(summary.problems.iter().all(|p| p.position != Some(1)));
    let json = serde_json::to_string(&summary).unwrap();
    // Flagged questions show their stem and options, so the problems themselves must not
    // quote them, and the unflagged question stays hidden.
    assert!(!json.contains("OPCAO-q1"), "{json}");
    assert!(!json.contains("EXPLICACAO"), "{json}");
    assert!(
        summary
            .problems
            .iter()
            .all(|p| !p.message.contains("OPCAO"))
    );
}

#[test]
fn a_draft_without_a_check_shows_every_question_unchecked() {
    let db = Database::open_in_memory().unwrap();
    insert(&db, &draft_of("d1", vec![question("q1", "a")], None));
    let summary = summary_of(&db, "d1");
    assert_eq!(summary.unchecked, 1);
    assert_eq!(summary.lines[0].state, LineState::Unchecked);
    assert_eq!(summary.check_failed, None);

    let failed = draft_of("d2", vec![question("q1", "a")], None);
    insert(&db, &failed);
    blind_check::record_failure(&db, &DraftId::from("d2"), "A conferência falhou.").unwrap();
    assert_eq!(
        summary_of(&db, "d2").check_failed.as_deref(),
        Some("A conferência falhou.")
    );
}

#[test]
fn import_all_makes_a_generated_collection_in_package_order() {
    let db = Database::open_in_memory().unwrap();
    mixed(&db);
    let result = import_with(&db, "d1", ImportMode::All).unwrap();
    assert_eq!((result.imported, result.merged, result.skipped), (3, 0, 0));
    assert!(result.title.starts_with(
        "Direito › Constitucional › Controle de constitucionalidade · gerado em 19/11/2026"
    ));

    let (kind, board, items) = db
        .with(|conn| {
            let (kind, board): (String, Option<String>) = conn.query_row(
                "SELECT kind, board FROM collections WHERE id = ?1",
                [&result.collection_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let items: i64 = conn.query_row(
                "SELECT COUNT(*) FROM collection_items WHERE collection_id = ?1",
                [&result.collection_id],
                |r| r.get(0),
            )?;
            Ok((kind, board, items))
        })
        .unwrap();
    assert_eq!(
        (kind.as_str(), board.as_deref(), items),
        ("generated", Some("Cebraspe"), 3)
    );
    assert_eq!(count(&db, "SELECT COUNT(*) FROM questions"), 3);
    // Every question is under the topic the user chose.
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM question_topics qt JOIN topics t ON t.id = qt.topic_id WHERE t.topic = 'Controle de constitucionalidade'"
        ),
        3
    );
    assert_eq!(summary_of(&db, "d1").status, DraftStatus::Imported);
    // Only the disputed question is marked as such.
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM questions WHERE json_extract(provenance, '$.disputed') = 1"
        ),
        1
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM questions WHERE json_extract(provenance, '$.draftId') = 'd1'"
        ),
        3
    );
}

#[test]
fn import_confirmed_leaves_out_disputed_and_unchecked_questions() {
    let db = Database::open_in_memory().unwrap();
    mixed(&db);
    let result = import_with(&db, "d1", ImportMode::Confirmed).unwrap();
    assert_eq!((result.imported, result.skipped), (2, 1));
    assert_eq!(count(&db, "SELECT COUNT(*) FROM questions"), 2);
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM questions WHERE json_extract(provenance, '$.disputed') = 1"
        ),
        0
    );

    insert(&db, &draft_of("d2", vec![question("q9", "a")], None));
    let error = import_with(&db, "d2", ImportMode::Confirmed).unwrap_err();
    assert!(error.to_string().contains("Nenhuma questão confirmada"));
    assert_eq!(summary_of(&db, "d2").status, DraftStatus::Review);
}

#[test]
fn a_package_with_problems_blocks_import_all_but_not_the_clean_confirmed_ones() {
    let db = Database::open_in_memory().unwrap();
    let mut broken = question("q2", "b");
    broken["options"].as_array_mut().unwrap().pop();
    insert(
        &db,
        &draft_of(
            "d1",
            vec![question("q1", "a"), broken],
            Some(vec![
                verdict("q1", VerdictState::Agreed),
                verdict("q2", VerdictState::Agreed),
            ]),
        ),
    );
    let error = import_with(&db, "d1", ImportMode::All).unwrap_err();
    assert!(error.to_string().contains("tem problemas"));
    assert_eq!(count(&db, "SELECT COUNT(*) FROM questions"), 0);

    let result = import_with(&db, "d1", ImportMode::Confirmed).unwrap();
    assert_eq!((result.imported, result.skipped), (1, 1));
}

#[test]
fn a_question_already_in_the_bank_joins_the_collection_without_duplicating() {
    let db = Database::open_in_memory().unwrap();
    insert(
        &db,
        &draft_of(
            "d1",
            vec![question("q1", "a")],
            Some(vec![verdict("q1", VerdictState::Agreed)]),
        ),
    );
    import_with(&db, "d1", ImportMode::All).unwrap();

    insert(
        &db,
        &draft_of(
            "d2",
            vec![question("q1", "a"), question("q2", "b")],
            Some(vec![
                verdict("q1", VerdictState::Agreed),
                verdict("q2", VerdictState::Agreed),
            ]),
        ),
    );
    assert_eq!(summary_of(&db, "d2").duplicates, 1);
    let result = import_with(&db, "d2", ImportMode::All).unwrap();
    assert_eq!((result.imported, result.merged), (1, 1));
    assert_eq!(count(&db, "SELECT COUNT(*) FROM questions"), 2);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM collection_items"), 3);
}

#[test]
fn a_key_that_conflicts_with_the_bank_is_a_problem_without_quoting_the_question() {
    let db = Database::open_in_memory().unwrap();
    insert(
        &db,
        &draft_of(
            "d1",
            vec![question("q1", "a")],
            Some(vec![verdict("q1", VerdictState::Agreed)]),
        ),
    );
    import_with(&db, "d1", ImportMode::All).unwrap();

    insert(
        &db,
        &draft_of(
            "d2",
            vec![question("q1", "b")],
            Some(vec![verdict("q1", VerdictState::Agreed)]),
        ),
    );
    let summary = summary_of(&db, "d2");
    assert!(
        summary
            .problems
            .iter()
            .any(|p| p.message.contains("resposta divergente"))
    );
    let error = import_with(&db, "d2", ImportMode::All).unwrap_err();
    assert!(!error.to_string().contains("ENUNCIADO"));
}

#[test]
fn a_package_whose_check_is_running_cannot_be_imported_or_discarded() {
    let db = Database::open_in_memory().unwrap();
    let mut draft = draft_of("d1", vec![question("q1", "a")], None);
    draft.blind_check = Some(pending_record());
    insert(&db, &draft);
    let summary = summary_of(&db, "d1");
    assert!(summary.checking);
    assert_eq!(summary.lines[0].state, LineState::Unchecked);
    assert!(db.with(|conn| waiting_items(conn)).unwrap()[0].checking);
    for mode in [ImportMode::All, ImportMode::Confirmed] {
        let error = import_with(&db, "d1", mode).unwrap_err();
        assert!(error.to_string().contains("ainda está em andamento"));
    }
    assert!(discard_with(&db, &draft.id).is_err());
    assert_eq!(count(&db, "SELECT COUNT(*) FROM questions"), 0);
    assert_eq!(summary_of(&db, "d1").status, DraftStatus::Review);
}

#[test]
fn importing_everything_needs_every_question_checked() {
    let db = Database::open_in_memory().unwrap();
    insert(&db, &draft_of("d1", vec![question("q1", "a")], None));
    let error = import_with(&db, "d1", ImportMode::All).unwrap_err();
    assert!(error.to_string().contains("sem conferência"));
    assert_eq!(count(&db, "SELECT COUNT(*) FROM questions"), 0);
    assert_eq!(summary_of(&db, "d1").status, DraftStatus::Review);
}

#[test]
fn a_package_with_quotes_in_its_options_leaks_nothing_in_the_preview() {
    let db = Database::open_in_memory().unwrap();
    let mut repeated = question("q1", "a");
    repeated["options"][1]["text"] = json!("O \"princípio\" OPCAO-SEGREDO");
    repeated["options"][2]["text"] = json!("O \"princípio\" OPCAO-SEGREDO");
    insert(
        &db,
        &draft_of(
            "d1",
            vec![repeated],
            Some(vec![verdict("q1", VerdictState::Agreed)]),
        ),
    );
    // The question is flagged, so its options are shown as content; the problem text
    // that describes it must still quote nothing.
    let summary = summary_of(&db, "d1");
    let shown = serde_json::to_string(&summary.problems).unwrap();
    assert!(shown.contains("mesmo texto"));
    assert!(
        !shown.contains("OPCAO-SEGREDO") && !shown.contains("princípio"),
        "{shown}"
    );
    // The whole summary never carries the key, explanation, references or checker reason.
    let all = serde_json::to_string(&summary).unwrap();
    for secret in ["EXPLICACAO", "REFERENCIA", "answerId", "RAZAO-SECRETA"] {
        assert!(!all.contains(secret), "{secret} in {all}");
    }
}

#[test]
fn flagged_content_has_exactly_the_documented_keys() {
    let db = Database::open_in_memory().unwrap();
    insert(
        &db,
        &draft_of(
            "d1",
            vec![question("q1", "a")],
            Some(vec![verdict("q1", VerdictState::Disputed)]),
        ),
    );
    let value = serde_json::to_value(summary_of(&db, "d1")).unwrap();
    let content = value["lines"][0]["content"].as_object().unwrap();
    let mut keys: Vec<_> = content.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["options", "stem", "type"]);
    assert_eq!(content["type"], "multiple");
    let option = content["options"][0].as_object().unwrap();
    let mut keys: Vec<_> = option.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["id", "text"]);
}

#[test]
fn a_draft_is_imported_or_discarded_only_once() {
    let db = Database::open_in_memory().unwrap();
    mixed(&db);
    import_with(&db, "d1", ImportMode::All).unwrap();
    assert!(
        import_with(&db, "d1", ImportMode::All)
            .unwrap_err()
            .to_string()
            .contains("já foi importado")
    );
    assert!(discard_with(&db, &DraftId::from("d1")).is_err());

    insert(&db, &draft_of("d2", vec![question("q7", "a")], None));
    discard_with(&db, &DraftId::from("d2")).unwrap();
    assert_eq!(summary_of(&db, "d2").status, DraftStatus::Discarded);
    assert!(import_with(&db, "d2", ImportMode::All).is_err());
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM questions WHERE json_extract(provenance, '$.draftId') = 'd2'"
        ),
        0
    );
}

fn keep_attachment(root: &Path, id: &DraftId) -> std::path::PathBuf {
    let source = root.join("source.md");
    std::fs::write(&source, "Synthetic attachment").unwrap();
    draft_files::keep(root, id, &[source]).unwrap().remove(0)
}

#[test]
fn the_summary_counts_confirmed_questions_and_flags_an_unfinished_check() {
    let db = Database::open_in_memory().unwrap();
    let draft = draft_of(
        "d1",
        vec![
            question("q1", "a"),
            question("q2", "a"),
            question("q3", "a"),
        ],
        Some(vec![
            verdict("q1", VerdictState::Agreed),
            verdict("q2", VerdictState::Fixed),
        ]),
    );
    insert(&db, &draft);

    let summary = summary_of(&db, "d1");

    assert_eq!((summary.confirmed, summary.needs_check), (2, true));
}

#[test]
fn a_checked_draft_imports_every_question_without_running_the_checker_again() {
    let db = Database::open_in_memory().unwrap();
    let files = tempfile::tempdir().unwrap();
    let draft = mixed(&db);

    let outcome = import_all(
        &db,
        files.path(),
        &draft.id,
        &GenerationManager::default(),
        || NOW.into(),
        |_, _| panic!("the draft was already fully checked"),
        |_| {},
    )
    .unwrap();

    let ImportAllOutcome::Imported { result } = outcome else {
        panic!("expected an import");
    };
    assert_eq!((result.imported, result.skipped), (3, 0));
}

#[test]
fn a_whole_draft_import_refuses_to_check_while_the_generation_slot_is_held() {
    let db = Database::open_in_memory().unwrap();
    let files = tempfile::tempdir().unwrap();
    let draft = draft_of("d1", vec![question("q1", "a")], None);
    insert(&db, &draft);
    let manager = GenerationManager::default();

    manager
        .run_check_then_import(
            NOW.into(),
            |_, _| {
                let error = import_all(
                    &db,
                    files.path(),
                    &draft.id,
                    &manager,
                    || NOW.into(),
                    |_, _| panic!("the generation slot is occupied"),
                    |_| {},
                )
                .unwrap_err();
                assert_eq!(
                    error.to_string(),
                    "Já existe uma geração em andamento. Espere terminar ou cancele."
                );
                Ok(())
            },
            |_| {},
        )
        .unwrap();

    assert_eq!(summary_of(&db, "d1").status, DraftStatus::Review);
}

#[test]
fn a_whole_draft_import_consults_the_check_state_at_write_time() {
    let db = Database::open_in_memory().unwrap();
    let files = tempfile::tempdir().unwrap();
    let draft = mixed(&db);
    assert!(!summary_of(&db, "d1").needs_check);
    let first_clock_read = std::cell::Cell::new(true);

    let result = import_all(
        &db,
        files.path(),
        &draft.id,
        &GenerationManager::default(),
        || {
            // A retry completed unsuccessfully after the preview read the old verdicts.
            if first_clock_read.replace(false) {
                blind_check::record_failure(&db, &draft.id, "Conferência indisponível.").unwrap();
            }
            NOW.into()
        },
        |_, _| {
            assert_eq!(count(&db, "SELECT COUNT(*) FROM questions"), 0);
            blind_check::record_failure(&db, &draft.id, "A conferência falhou de novo.")
        },
        |_| {},
    )
    .unwrap();

    assert_eq!(
        result,
        ImportAllOutcome::Pending {
            message:
                "A conferência não terminou: A conferência falhou de novo. Nada foi importado."
                    .into(),
        }
    );
    assert_eq!(summary_of(&db, "d1").status, DraftStatus::Review);
}

#[test]
fn the_generation_slot_stays_held_until_the_checked_draft_is_imported() {
    let db = Database::open_in_memory().unwrap();
    let files = tempfile::tempdir().unwrap();
    let draft = draft_of("d1", vec![question("q1", "a")], None);
    let checked = draft_of(
        "d1",
        vec![question("q1", "a")],
        Some(vec![verdict("q1", VerdictState::Agreed)]),
    );
    insert(&db, &draft);
    let manager = GenerationManager::default();
    let events = std::cell::RefCell::new(Vec::new());

    let outcome = import_all(
        &db,
        files.path(),
        &draft.id,
        &manager,
        || NOW.into(),
        |_, _| {
            assert!(manager.status().running);
            db.with(|conn| draft::set_blind_check(conn, &draft.id, &checked.blind_check.unwrap()))
        },
        |status| {
            events.borrow_mut().push((
                status.running,
                status.can_cancel,
                manager.cancel(),
                summary_of(&db, "d1").status,
            ));
        },
    )
    .unwrap();

    assert!(matches!(outcome, ImportAllOutcome::Imported { .. }));
    assert_eq!(
        events.into_inner(),
        [
            (true, false, false, DraftStatus::Review),
            (false, false, false, DraftStatus::Imported),
        ]
    );
    assert_eq!(count(&db, "SELECT COUNT(*) FROM questions"), 1);
}

#[test]
fn a_cancelled_check_leaves_the_draft_waiting_for_review() {
    let db = Database::open_in_memory().unwrap();
    let files = tempfile::tempdir().unwrap();
    let draft = draft_of("d1", vec![question("q1", "a")], None);
    insert(&db, &draft);
    let manager = GenerationManager::default();

    let outcome = import_all(
        &db,
        files.path(),
        &draft.id,
        &manager,
        || NOW.into(),
        |cancel, _| {
            assert!(manager.cancel());
            assert!(cancel.is_cancelled());
            Ok(())
        },
        |_| {},
    )
    .unwrap();

    assert_eq!(
        outcome,
        ImportAllOutcome::Pending {
            message: "A conferência não terminou: A conferência foi cancelada. Nada foi importado."
                .into(),
        }
    );
    assert_eq!(summary_of(&db, "d1").status, DraftStatus::Review);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM questions"), 0);
}

#[test]
fn a_failed_check_reports_its_message_without_importing_any_questions() {
    let db = Database::open_in_memory().unwrap();
    let files = tempfile::tempdir().unwrap();
    let draft = draft_of("d1", vec![question("q1", "a")], None);
    insert(&db, &draft);

    let outcome = import_all(
        &db,
        files.path(),
        &draft.id,
        &GenerationManager::default(),
        || NOW.into(),
        |_, _| blind_check::record_failure(&db, &draft.id, "Conferência indisponível."),
        |_| {},
    )
    .unwrap();

    assert_eq!(
        outcome,
        ImportAllOutcome::Pending {
            message: "A conferência não terminou: Conferência indisponível. Nada foi importado."
                .into(),
        }
    );
    assert_eq!(count(&db, "SELECT COUNT(*) FROM questions"), 0);
}

#[test]
fn importing_removes_the_kept_attachments() {
    let db = Database::open_in_memory().unwrap();
    let files = tempfile::tempdir().unwrap();
    let draft = mixed(&db);
    let kept = keep_attachment(files.path(), &draft.id);

    import(&db, files.path(), &draft.id, ImportMode::All, NOW).unwrap();

    assert!(!kept.exists());
}

#[test]
fn discarding_removes_the_kept_attachments() {
    let db = Database::open_in_memory().unwrap();
    let files = tempfile::tempdir().unwrap();
    let draft = mixed(&db);
    let kept = keep_attachment(files.path(), &draft.id);

    discard(&db, files.path(), &draft.id).unwrap();

    assert!(!kept.exists());
}

#[test]
fn a_failed_import_commit_leaves_the_kept_attachments() {
    let db = Database::open_in_memory().unwrap();
    let files = tempfile::tempdir().unwrap();
    let draft = mixed(&db);
    let kept = keep_attachment(files.path(), &draft.id);
    db.with(|conn| {
        conn.execute_batch(
            "CREATE TABLE commit_guard (
                question_id TEXT REFERENCES questions(id) DEFERRABLE INITIALLY DEFERRED
             );
             CREATE TRIGGER refuse_draft_commit AFTER UPDATE OF status ON drafts BEGIN
                INSERT INTO commit_guard VALUES ('missing-question');
             END;",
        )?;
        Ok(())
    })
    .unwrap();

    assert!(import(&db, files.path(), &draft.id, ImportMode::All, NOW).is_err());

    assert!(kept.is_file());
    assert_eq!(summary_of(&db, "d1").status, DraftStatus::Review);
}

#[test]
fn the_waiting_list_has_only_drafts_to_review() {
    let db = Database::open_in_memory().unwrap();
    mixed(&db);
    insert(&db, &draft_of("d2", vec![question("q7", "a")], None));
    insert(&db, &draft_of("d3", vec![question("q8", "a")], None));
    discard_with(&db, &DraftId::from("d3")).unwrap();
    let list = db.with(|conn| waiting(conn)).unwrap();
    let mut ids: Vec<_> = list.iter().map(|d| d.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["d1", "d2"]);
    assert_eq!(
        list.iter()
            .find(|d| d.id.as_str() == "d1")
            .unwrap()
            .disputed,
        1
    );
    let json = serde_json::to_string(&list).unwrap();
    assert!(!json.contains("ENUNCIADO"));
}
