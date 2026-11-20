use super::*;
use crate::ai::blind_check::{BlindCheck, Confidence, Verdict};
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
        blind_check: verdicts.map(|verdicts| {
            serde_json::to_value(BlindCheck {
                verdicts,
                failed: None,
                info: None,
            })
            .unwrap()
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
    db.with(|conn| summary(conn, id)).unwrap()
}

fn import_with(db: &Database, id: &str, mode: ImportMode) -> AppResult<ImportResult> {
    db.transaction(|tx| import(tx, id, mode, NOW))
}

fn count(db: &Database, sql: &str) -> i64 {
    db.with(|conn| Ok(conn.query_row(sql, [], |row| row.get(0))?))
        .unwrap()
}

#[test]
fn the_summary_shows_counts_and_states_but_no_content() {
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
            .map(|l| (l.position, l.state))
            .collect::<Vec<_>>(),
        [
            (1, LineState::Agreed),
            (2, LineState::Disputed),
            (3, LineState::Fixed)
        ]
    );
    assert_eq!(summary.board.as_deref(), Some("Cebraspe"));

    let json = serde_json::to_string(&summary).unwrap();
    for secret in [
        "ENUNCIADO",
        "OPCAO",
        "EXPLICACAO",
        "REFERENCIA",
        "RAZAO-SECRETA",
        "answerId",
        "stem",
        "explanation",
        "options",
    ] {
        assert!(
            !json.contains(secret),
            "the preview must not contain {secret:?}: {json}"
        );
    }
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
    assert!(!json.contains("OPCAO"), "{json}");
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
    blind_check::record_failure(&db, "d2", "A conferência falhou.").unwrap();
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
            .any(|p| p.message.contains("outro gabarito"))
    );
    let error = import_with(&db, "d2", ImportMode::All).unwrap_err();
    assert!(!error.to_string().contains("ENUNCIADO"));
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
    assert!(db.with(|conn| discard(conn, "d1")).is_err());

    insert(&db, &draft_of("d2", vec![question("q7", "a")], None));
    db.with(|conn| discard(conn, "d2")).unwrap();
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

#[test]
fn the_waiting_list_has_only_drafts_to_review() {
    let db = Database::open_in_memory().unwrap();
    mixed(&db);
    insert(&db, &draft_of("d2", vec![question("q7", "a")], None));
    insert(&db, &draft_of("d3", vec![question("q8", "a")], None));
    db.with(|conn| discard(conn, "d3")).unwrap();
    let list = db.with(|conn| waiting(conn)).unwrap();
    let mut ids: Vec<_> = list.iter().map(|d| d.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["d1", "d2"]);
    assert_eq!(list.iter().find(|d| d.id == "d1").unwrap().disputed, 1);
    let json = serde_json::to_string(&list).unwrap();
    assert!(!json.contains("ENUNCIADO"));
}
