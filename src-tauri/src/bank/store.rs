//! Reading and writing questions in the database, including deduplicating imports.
//!
//! An import is planned first ([`plan_import`]: validation, canonicalization, conflicts) and
//! then applied ([`apply_import`]) inside the caller's transaction. A plan with problems is
//! never applied, so a bad package changes nothing.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};

use super::identity::js_trim;
use super::question::{
    Provenance, Question, QuestionInput, QuestionOption, QuestionType, TopicPath, canonicalize,
};
use crate::error::{AppError, AppResult};
use crate::ids::{QuestionId, TopicId};

/// One canonical question of a planned import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedQuestion {
    /// Every input (by local ID) that resolves to this question.
    pub local_ids: Vec<String>,
    pub question: Question,
    /// Whether a question with this fingerprint is already in the bank.
    pub existing: bool,
}

/// The result of checking a batch of questions against the rules and the bank.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportPlan {
    pub questions: Vec<PlannedQuestion>,
    /// pt-BR problems. Any problem blocks the import.
    pub problems: Vec<String>,
}

impl ImportPlan {
    /// Canonical question ID for each input's local ID.
    pub fn ids_by_local_id(&self) -> HashMap<String, QuestionId> {
        self.questions
            .iter()
            .flat_map(|p| {
                p.local_ids
                    .iter()
                    .map(|l| (l.clone(), p.question.id.clone()))
            })
            .collect()
    }

    pub fn existing_count(&self) -> usize {
        self.questions.iter().filter(|p| p.existing).count()
    }
}

/// What [`apply_import`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOutcome {
    pub inserted: usize,
    pub merged: usize,
    /// Canonical question ID for each input's local ID.
    pub ids_by_local_id: HashMap<String, QuestionId>,
}

fn excerpt(text: &str) -> String {
    let trimmed = js_trim(text);
    let short: String = trimmed.chars().take(90).collect();
    if short.chars().count() < trimmed.chars().count() {
        format!("{short}…")
    } else {
        short
    }
}

/// Merges `incoming` into `kept` (same fingerprint, same answer): keeps the existing
/// explanation if there is one, and unions references and topics.
fn merge_into(kept: &mut Question, incoming: &Question) {
    if js_trim(&kept.explanation).is_empty() {
        kept.explanation.clone_from(&incoming.explanation);
    }
    for reference in &incoming.refs {
        if !kept.refs.contains(reference) {
            kept.refs.push(reference.clone());
        }
    }
    for topic in &incoming.topics {
        if !kept.topics.contains(topic) {
            kept.topics.push(topic.clone());
        }
    }
    if kept.image_alt.is_none() {
        kept.image_alt.clone_from(&incoming.image_alt);
    }
}

/// Validates and canonicalizes `inputs`, merges duplicates within the batch and checks each
/// question against the bank. Writes nothing.
pub fn plan_import(conn: &Connection, inputs: Vec<QuestionInput>) -> AppResult<ImportPlan> {
    let mut plan = ImportPlan::default();
    let mut index_by_id: HashMap<QuestionId, usize> = HashMap::new();
    let mut seen_local_ids: HashMap<String, ()> = HashMap::new();

    for input in inputs {
        let local_id = input.local_id.clone();
        if seen_local_ids.insert(local_id.clone(), ()).is_some() {
            plan.problems.push(format!(
                "Questão {local_id}: identificador repetido no pacote."
            ));
            continue;
        }
        let question = match canonicalize(input) {
            Ok(question) => question,
            Err(problems) => {
                plan.problems.extend(
                    problems
                        .into_iter()
                        .map(|p| format!("Questão {local_id}: {p}")),
                );
                continue;
            }
        };
        match index_by_id.get(&question.id) {
            Some(&index) => {
                let planned = &mut plan.questions[index];
                if planned.question.answer_id != question.answer_id {
                    plan.problems.push(format!(
                        "Questões {} e {local_id} são iguais, mas têm gabaritos diferentes: \"{}\".",
                        planned.local_ids[0],
                        excerpt(&question.stem)
                    ));
                    continue;
                }
                planned.local_ids.push(local_id);
                merge_into(&mut planned.question, &question);
            }
            None => {
                index_by_id.insert(question.id.clone(), plan.questions.len());
                plan.questions.push(PlannedQuestion {
                    local_ids: vec![local_id],
                    question,
                    existing: false,
                });
            }
        }
    }

    for planned in &mut plan.questions {
        if let Some(stored) = load_question(conn, &planned.question.id)? {
            planned.existing = true;
            if stored.answer_id != planned.question.answer_id {
                plan.problems.push(format!(
                    "Questão {}: já existe no banco com outro gabarito: \"{}\".",
                    planned.local_ids[0],
                    excerpt(&planned.question.stem)
                ));
            }
        }
    }
    Ok(plan)
}

/// Writes a plan: inserts new questions and merges the ones already in the bank.
/// Refuses a plan with problems. Call inside a transaction.
pub fn apply_import(conn: &Connection, plan: &ImportPlan, now: &str) -> AppResult<ImportOutcome> {
    if !plan.problems.is_empty() {
        return Err(AppError::Invalid(format!(
            "Resolva os problemas do pacote antes de importar:\n{}",
            plan.problems.join("\n")
        )));
    }
    let mut inserted = 0;
    let mut merged = 0;
    for planned in &plan.questions {
        match load_question(conn, &planned.question.id)? {
            Some(mut stored) => {
                if stored.answer_id != planned.question.answer_id {
                    return Err(AppError::Invalid(format!(
                        "A questão \"{}\" mudou de gabarito no banco durante a importação.",
                        excerpt(&planned.question.stem)
                    )));
                }
                let before = stored.clone();
                merge_into(&mut stored, &planned.question);
                if stored != before {
                    update_question(conn, &stored, now)?;
                }
                merged += 1;
            }
            None => {
                insert_question(conn, &planned.question, now)?;
                inserted += 1;
            }
        }
    }
    Ok(ImportOutcome {
        inserted,
        merged,
        ids_by_local_id: plan.ids_by_local_id(),
    })
}

/// Plans and applies in one step. Fails, writing nothing, if the plan has problems.
pub fn import_questions(
    conn: &Connection,
    inputs: Vec<QuestionInput>,
    now: &str,
) -> AppResult<ImportOutcome> {
    let plan = plan_import(conn, inputs)?;
    apply_import(conn, &plan, now)
}

/// The stems of every question under exactly this topic, oldest first. The generator gets them
/// so it avoids near-duplicates (spec §2 decision 9).
pub fn topic_stems(conn: &Connection, path: &TopicPath) -> AppResult<Vec<String>> {
    let mut statement = conn.prepare(
        "SELECT q.stem FROM questions q
         JOIN question_topics qt ON qt.question_id = q.id
         JOIN topics t ON t.id = qt.topic_id
         WHERE t.area = ?1 AND t.matter = ?2 AND t.topic = ?3
         ORDER BY q.created_at, q.id",
    )?;
    let stems = statement
        .query_map(params![path.area, path.matter, path.topic], |row| {
            row.get(0)
        })?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(stems)
}

/// Returns the topic's ID, creating it if needed.
pub fn upsert_topic(conn: &Connection, path: &TopicPath) -> AppResult<TopicId> {
    conn.execute(
        "INSERT INTO topics (area, matter, topic) VALUES (?1, ?2, ?3)
         ON CONFLICT (area, matter, topic) DO NOTHING",
        params![path.area, path.matter, path.topic],
    )?;
    Ok(conn.query_row(
        "SELECT id FROM topics WHERE area = ?1 AND matter = ?2 AND topic = ?3",
        params![path.area, path.matter, path.topic],
        |row| row.get(0),
    )?)
}

fn write_topics(conn: &Connection, question: &Question) -> AppResult<()> {
    conn.execute(
        "DELETE FROM question_topics WHERE question_id = ?1",
        params![question.id],
    )?;
    for path in &question.topics {
        let topic_id = upsert_topic(conn, path)?;
        conn.execute(
            "INSERT INTO question_topics (question_id, topic_id) VALUES (?1, ?2)",
            params![question.id, topic_id],
        )?;
    }
    Ok(())
}

fn insert_question(conn: &Connection, q: &Question, now: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO questions (id, type, stem, options, answer_id, explanation, refs, image,
                                image_alt, annulled, provenance, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
        params![
            q.id,
            q.kind,
            q.stem,
            serde_json::to_string(&q.options)?,
            q.answer_id,
            q.explanation,
            serde_json::to_string(&q.refs)?,
            q.image,
            q.image_alt,
            q.annulled,
            serde_json::to_string(&q.provenance)?,
            now,
        ],
    )?;
    write_topics(conn, q)
}

fn update_question(conn: &Connection, q: &Question, now: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE questions SET explanation = ?2, refs = ?3, image_alt = ?4, updated_at = ?5
         WHERE id = ?1",
        params![
            q.id,
            q.explanation,
            serde_json::to_string(&q.refs)?,
            q.image_alt,
            now,
        ],
    )?;
    write_topics(conn, q)
}

/// Loads a question with its topics (in topic ID order).
pub fn load_question(conn: &Connection, id: &QuestionId) -> AppResult<Option<Question>> {
    let row = conn
        .query_row(
            "SELECT type, stem, options, answer_id, explanation, refs, image, image_alt,
                    annulled, provenance
             FROM questions WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, QuestionType>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, bool>(8)?,
                    row.get::<_, String>(9)?,
                ))
            },
        )
        .optional()?;
    let Some((
        kind,
        stem,
        options,
        answer_id,
        explanation,
        refs,
        image,
        image_alt,
        annulled,
        provenance,
    )) = row
    else {
        return Ok(None);
    };
    let mut stmt = conn.prepare_cached(
        "SELECT t.area, t.matter, t.topic FROM question_topics qt
         JOIN topics t ON t.id = qt.topic_id
         WHERE qt.question_id = ?1 ORDER BY t.id",
    )?;
    let topics = stmt
        .query_map(params![id], |row| {
            Ok(TopicPath {
                area: row.get(0)?,
                matter: row.get(1)?,
                topic: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(Question {
        id: id.to_owned(),
        kind,
        stem,
        options: serde_json::from_str::<Vec<QuestionOption>>(&options)?,
        answer_id,
        explanation,
        refs: serde_json::from_str(&refs)?,
        topics,
        image,
        image_alt,
        annulled,
        provenance: serde_json::from_str::<Provenance>(&provenance)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank::question::tests::{boolean, multiple, topic};
    use crate::db::Database;

    const NOW: &str = "2026-11-19T12:00:00.000Z";
    const LATER: &str = "2026-11-20T12:00:00.000Z";

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn import_stores_canonical_questions_with_their_topics() {
        let db = Database::open_in_memory().unwrap();
        db.transaction(|tx| {
            let outcome = import_questions(
                tx,
                vec![
                    multiple("q1", "Primeira", 3),
                    boolean("q2", "Segunda", false),
                ],
                NOW,
            )?;
            assert_eq!((outcome.inserted, outcome.merged), (2, 0));
            let id = &outcome.ids_by_local_id["q1"];
            let stored = load_question(tx, id)?.expect("stored");
            assert_eq!(stored.answer_text(), Some("Delta"));
            assert_eq!(stored.answer_id, crate::bank::identity::option_id("Delta"));
            assert_eq!(stored.topics, vec![topic()]);
            assert_eq!(stored.provenance, Provenance::Legacy);
            assert_eq!(count(tx, "topics"), 1);
            assert_eq!(count(tx, "question_topics"), 2);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn import_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        let batch = || {
            vec![
                multiple("q1", "Primeira", 0),
                boolean("q2", "Segunda", true),
            ]
        };
        db.transaction(|tx| import_questions(tx, batch(), NOW))
            .unwrap();
        let snapshot = |db: &Database| {
            db.with(|conn| {
                let mut stmt = conn.prepare("SELECT * FROM questions ORDER BY id")?;
                let rows = stmt
                    .query_map([], |row| {
                        (0..13)
                            .map(|i| row.get::<_, rusqlite::types::Value>(i))
                            .collect::<Result<Vec<_>, _>>()
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((rows, count(conn, "question_topics"), count(conn, "topics")))
            })
            .unwrap()
        };
        let before = snapshot(&db);

        let outcome = db
            .transaction(|tx| import_questions(tx, batch(), LATER))
            .unwrap();
        assert_eq!((outcome.inserted, outcome.merged), (0, 2));
        assert_eq!(
            snapshot(&db),
            before,
            "nothing changes, not even updated_at"
        );
    }

    #[test]
    fn options_are_normalized_whatever_ids_the_input_uses() {
        let db = Database::open_in_memory().unwrap();
        let mut renamed = multiple("x", "Primeira", 1);
        for (i, option) in renamed.options.iter_mut().enumerate() {
            option.id = format!("letra-{i}");
        }
        renamed.answer_id = "letra-1".into();
        renamed.options.reverse();

        db.transaction(|tx| {
            let first = import_questions(tx, vec![multiple("q1", "Primeira", 1)], NOW)?;
            let second = import_questions(tx, vec![renamed], LATER)?;
            assert_eq!(first.ids_by_local_id["q1"], second.ids_by_local_id["x"]);
            assert_eq!((second.inserted, second.merged), (0, 1));
            assert_eq!(count(tx, "questions"), 1);
            Ok(())
        })
        .unwrap();
    }

    fn merge_duplicate(original: QuestionInput, copy: QuestionInput) -> Question {
        let db = Database::open_in_memory().unwrap();
        let copy_id = copy.local_id.clone();
        db.transaction(|tx| {
            import_questions(tx, vec![original], NOW)?;
            let outcome = import_questions(tx, vec![copy], LATER)?;
            Ok(load_question(tx, &outcome.ids_by_local_id[&copy_id])?.unwrap())
        })
        .unwrap()
    }

    #[test]
    fn a_duplicate_fills_a_missing_explanation() {
        let original = multiple("q1", "Primeira", 0);
        let mut copy = multiple("q2", "Primeira", 0);
        copy.explanation = "Porque sim.".into();

        assert_eq!(merge_duplicate(original, copy).explanation, "Porque sim.");
    }

    #[test]
    fn a_duplicate_keeps_an_existing_explanation() {
        let mut original = multiple("q1", "Primeira", 0);
        original.explanation = "Porque sim.".into();
        let mut copy = multiple("q2", "Primeira", 0);
        copy.explanation = "Outra explicação.".into();

        assert_eq!(merge_duplicate(original, copy).explanation, "Porque sim.");
    }

    #[test]
    fn a_duplicate_merges_references() {
        let mut original = multiple("q1", "Primeira", 0);
        original.refs = vec!["CF/88".into()];
        let mut copy = multiple("q2", "Primeira", 0);
        copy.refs = vec!["CF/88".into(), "STF, ADI 4.277".into()];

        assert_eq!(
            merge_duplicate(original, copy).refs,
            ["CF/88", "STF, ADI 4.277"]
        );
    }

    #[test]
    fn a_duplicate_merges_topics() {
        let original = multiple("q1", "Primeira", 0);
        let mut copy = multiple("q2", "Primeira", 0);
        let another_topic = TopicPath::new("Direito", "Constitucional", "Direitos fundamentais");
        copy.topics = vec![another_topic.clone()];

        assert_eq!(
            merge_duplicate(original, copy).topics,
            [topic(), another_topic]
        );
    }

    #[test]
    fn answer_key_conflict_with_the_bank_blocks_the_whole_import() {
        let db = Database::open_in_memory().unwrap();
        db.transaction(|tx| import_questions(tx, vec![multiple("q1", "Primeira", 0)], NOW))
            .unwrap();

        let error = db
            .transaction(|tx| {
                import_questions(
                    tx,
                    vec![multiple("novo", "Nova", 0), multiple("q1b", "Primeira", 4)],
                    LATER,
                )
            })
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("já existe no banco com outro gabarito"),
            "{error}"
        );
        db.with(|conn| {
            assert_eq!(
                count(conn, "questions"),
                1,
                "the valid question was not imported either"
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn answer_key_conflict_inside_a_batch_is_a_problem() {
        let db = Database::open_in_memory().unwrap();
        let plan = db
            .with(|conn| {
                plan_import(
                    conn,
                    vec![multiple("a", "Igual", 0), multiple("b", "Igual", 1)],
                )
            })
            .unwrap();
        assert_eq!(plan.problems.len(), 1);
        assert!(
            plan.problems[0].contains("Questões a e b são iguais"),
            "{:?}",
            plan.problems
        );
    }

    #[test]
    fn an_invalid_question_is_reported_with_its_local_id() {
        let db = Database::open_in_memory().unwrap();
        let mut broken = multiple("q7", "Enunciado", 0);
        broken.options.truncate(3);
        let plan = db
            .with(|conn| plan_import(conn, vec![multiple("ok", "Válida", 0), broken]))
            .unwrap();
        assert!(
            plan.problems.iter().all(|p| p.starts_with("Questão q7:")),
            "{:?}",
            plan.problems
        );
        assert_eq!(plan.questions.len(), 1);
    }

    #[test]
    fn an_invalid_question_blocks_the_import() {
        let db = Database::open_in_memory().unwrap();
        let mut broken = multiple("q7", "Enunciado", 0);
        broken.options.truncate(3);

        let error = db
            .transaction(|tx| import_questions(tx, vec![multiple("ok", "Válida", 0), broken], NOW))
            .unwrap_err();
        assert!(matches!(error, AppError::Invalid(_)));
        db.with(|conn| {
            assert_eq!(count(conn, "questions"), 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn repeated_local_ids_are_a_problem() {
        let db = Database::open_in_memory().unwrap();
        let plan = db
            .with(|conn| {
                plan_import(
                    conn,
                    vec![multiple("q", "Um", 0), boolean("q", "Dois", true)],
                )
            })
            .unwrap();
        assert_eq!(
            plan.problems,
            ["Questão q: identificador repetido no pacote."]
        );
    }

    #[test]
    fn plan_reports_questions_already_in_the_bank() {
        let db = Database::open_in_memory().unwrap();
        db.transaction(|tx| import_questions(tx, vec![multiple("q1", "Primeira", 0)], NOW))
            .unwrap();
        let plan = db
            .with(|conn| {
                plan_import(
                    conn,
                    vec![multiple("a", "Primeira", 0), multiple("b", "Nova", 0)],
                )
            })
            .unwrap();
        assert!(plan.problems.is_empty());
        assert_eq!(plan.existing_count(), 1);
    }
}
