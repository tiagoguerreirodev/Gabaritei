//! Sessions and attempts: starting, answering, eliminating, flagging, finishing, abandoning,
//! and the one home for outcome and scoring rules (spec §3.3, §3.4).
//!
//! **No spoilers (AGENTS.md rule 1).** [`AttemptView`] is the only shape an attempt takes on
//! its way to the frontend. A question's key, explanation and references live in
//! [`Reveal`], which is filled in only once that question is revealed: right after it is
//! answered in Study mode, or when the attempt is finished. Everything else in the view is
//! what the question card shows anyway.
//!
//! **Retroactive scoring.** A response stores a snapshot of the question *as shown* (stem and
//! shuffled options) and the choice. Key, explanation and annulment are always read from the
//! current question, so corrections and annulments re-score past attempts. No score is stored.

use rand::seq::SliceRandom;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bank::{QuestionOption, QuestionType};
use crate::error::{AppError, AppResult};

/// Largest topic session.
pub const MAX_TOPIC_SESSION: u32 = 200;

// ---- Requests --------------------------------------------------------------------------------

/// *Estudo* reveals each answer right away; *Simulado* reveals everything on finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum FeedbackMode {
    Study,
    Exam,
}

impl FeedbackMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Study => "study",
            Self::Exam => "exam",
        }
    }

    pub fn parse(value: &str) -> AppResult<Self> {
        match value {
            "study" => Ok(Self::Study),
            "exam" => Ok(Self::Exam),
            other => Err(AppError::Internal(format!(
                "unknown feedback mode {other:?}"
            ))),
        }
    }
}

/// Where a session's questions come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum SessionSource {
    /// Every question of a collection.
    Collection { collection_id: String },
    /// Up to `count` questions of a topic: unseen first, then least recently answered.
    Topic { topic_id: i64, count: u32 },
    /// The error notebook (#21).
    Errors,
    /// Due spaced-repetition reviews (#21).
    Reviews,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionStart {
    pub source: SessionSource,
    pub feedback_mode: FeedbackMode,
}

// ---- Views (what the frontend may see) ---------------------------------------------------------

/// Result of one question under the current key and annulment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Outcome {
    Correct,
    Wrong,
    Blank,
    Annulled,
}

/// The one scoring rule. Annulment wins over everything; otherwise blank, correct or wrong.
pub fn outcome(choice: Option<&str>, answer_id: &str, annulled: bool) -> Outcome {
    match choice {
        _ if annulled => Outcome::Annulled,
        None => Outcome::Blank,
        Some(choice) if choice == answer_id => Outcome::Correct,
        Some(_) => Outcome::Wrong,
    }
}

/// Totals for a set of outcomes. Annulled questions leave every denominator.
#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Score {
    pub correct: u32,
    pub wrong: u32,
    pub blank: u32,
    pub annulled: u32,
    /// Questions that count: everything but annulled ones.
    pub evaluated: u32,
    /// *Aproveitamento*: correct ÷ evaluated. `None` when nothing counts.
    pub achievement: Option<f64>,
    /// *Acerto entre respondidas*: correct ÷ (correct + wrong). `None` when nothing was answered.
    pub accuracy: Option<f64>,
}

impl Score {
    pub fn of(outcomes: impl IntoIterator<Item = Outcome>) -> Self {
        let mut score = Self::default();
        for outcome in outcomes {
            match outcome {
                Outcome::Correct => score.correct += 1,
                Outcome::Wrong => score.wrong += 1,
                Outcome::Blank => score.blank += 1,
                Outcome::Annulled => score.annulled += 1,
            }
        }
        score.evaluated = score.correct + score.wrong + score.blank;
        let ratio = |part: u32, whole: u32| (whole > 0).then(|| f64::from(part) / f64::from(whole));
        score.achievement = ratio(score.correct, score.evaluated);
        score.accuracy = ratio(score.correct, score.correct + score.wrong);
        score
    }
}

/// Key, explanation and result of a revealed question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Reveal {
    pub answer_id: String,
    pub explanation: String,
    pub refs: Vec<String>,
    pub annulled: bool,
    pub outcome: Outcome,
    /// The blind check still disagreed with the key when this generated question was imported.
    pub disputed: bool,
}

/// One question of an attempt, as the card shows it. `reveal` is `None` until revealed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ResponseView {
    pub position: u32,
    pub question_id: String,
    #[serde(rename = "type")]
    #[ts(rename = "type", type = "\"multiple\" | \"boolean\"")]
    pub kind: QuestionType,
    pub stem: String,
    /// In the order shown in this attempt.
    #[ts(type = "Array<{ id: string, text: string }>")]
    pub options: Vec<QuestionOption>,
    pub image: Option<String>,
    pub image_alt: Option<String>,
    pub choice: Option<String>,
    pub eliminated: Vec<String>,
    pub flagged: bool,
    pub reveal: Option<Reveal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SourceKind {
    Collection,
    Topic,
    Errors,
    Reviews,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AttemptView {
    pub id: String,
    pub source_kind: SourceKind,
    /// Collection ID or topic ID, depending on `source_kind`.
    pub source_ref: Option<String>,
    /// pt-BR title: the collection title, the topic path, "Caderno de erros" or "Revisões".
    pub title: String,
    pub feedback_mode: FeedbackMode,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// Bumped on every change; `finish` must quote the latest one.
    pub revision: u32,
    pub questions: Vec<ResponseView>,
    /// Filled in once the attempt is finished.
    pub score: Option<Score>,
}

// ---- Storage ---------------------------------------------------------------------------------

/// The question as shown in an attempt. Holds no key, explanation or annulment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    #[serde(rename = "type")]
    kind: QuestionType,
    stem: String,
    options: Vec<QuestionOption>,
    image: Option<String>,
    image_alt: Option<String>,
}

struct Header {
    source_kind: SourceKind,
    source_ref: Option<String>,
    mode: FeedbackMode,
    started_at: String,
    finished_at: Option<String>,
    revision: u32,
}

fn not_found() -> AppError {
    AppError::Invalid("Tentativa não encontrada.".into())
}

fn source_kind(value: &str) -> AppResult<SourceKind> {
    Ok(match value {
        "collection" => SourceKind::Collection,
        "topic" => SourceKind::Topic,
        "errors" => SourceKind::Errors,
        "reviews" => SourceKind::Reviews,
        other => return Err(AppError::Internal(format!("unknown source kind {other:?}"))),
    })
}

fn load_header(conn: &Connection, id: &str) -> AppResult<Header> {
    let row = conn
        .query_row(
            "SELECT source_kind, source_ref, feedback_mode, started_at, finished_at, revision
             FROM attempts WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, u32>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(not_found)?;
    Ok(Header {
        source_kind: source_kind(&row.0)?,
        source_ref: row.1,
        mode: FeedbackMode::parse(&row.2)?,
        started_at: row.3,
        finished_at: row.4,
        revision: row.5,
    })
}

fn new_attempt_id() -> String {
    let bytes: [u8; 16] = rand::random();
    hex::encode(bytes)
}

fn collection_title(conn: &Connection, id: &str) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT title FROM collections WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?)
}

fn topic_title(conn: &Connection, id: i64) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT area || ' › ' || matter || ' › ' || topic FROM topics WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?)
}

fn title(conn: &Connection, kind: SourceKind, source_ref: Option<&str>) -> AppResult<String> {
    let found = match (kind, source_ref) {
        (SourceKind::Collection, Some(id)) => collection_title(conn, id)?,
        (SourceKind::Topic, Some(id)) => match id.parse() {
            Ok(id) => topic_title(conn, id)?,
            Err(_) => None,
        },
        (SourceKind::Errors, _) => Some("Caderno de erros".into()),
        (SourceKind::Reviews, _) => Some("Revisões".into()),
        _ => None,
    };
    Ok(found.unwrap_or_else(|| "Sessão".into()))
}

/// Questions of a topic for a session: never answered first, then least recently answered;
/// annulled questions skipped. Ties are broken at random.
fn topic_questions(conn: &Connection, topic_id: i64, count: u32) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT q.id, MAX(r.answered_at)
         FROM question_topics qt
         JOIN questions q ON q.id = qt.question_id
         LEFT JOIN responses r ON r.question_id = q.id AND r.answered_at IS NOT NULL
         WHERE qt.topic_id = ?1 AND q.annulled = 0
         GROUP BY q.id",
    )?;
    let mut rows = stmt
        .query_map(params![topic_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.shuffle(&mut rand::rng());
    // `None` (never answered) sorts first; the sort is stable, so ties stay shuffled.
    rows.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(rows
        .into_iter()
        .take(count as usize)
        .map(|(id, _)| id)
        .collect())
}

/// Builds the shown snapshot of a question: options shuffled, except Certo/Errado, which
/// keeps its fixed order.
fn snapshot(conn: &Connection, question_id: &str) -> AppResult<Snapshot> {
    let (kind, stem, options, image, image_alt) = conn.query_row(
        "SELECT type, stem, options, image, image_alt FROM questions WHERE id = ?1",
        params![question_id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        },
    )?;
    let kind = QuestionType::parse(&kind)
        .ok_or_else(|| AppError::Internal(format!("unknown question type {kind:?}")))?;
    let mut options: Vec<QuestionOption> = serde_json::from_str(&options)?;
    if kind == QuestionType::Multiple {
        options.shuffle(&mut rand::rng());
    }
    Ok(Snapshot {
        kind,
        stem,
        options,
        image,
        image_alt,
    })
}

// ---- Operations ------------------------------------------------------------------------------

/// Starts a session and returns it. Questions and options are shuffled with the OS-seeded
/// CSPRNG behind `rand::rng()`.
pub fn start(conn: &Connection, request: &SessionStart, now: &str) -> AppResult<AttemptView> {
    let (kind, source_ref, mut questions) = match &request.source {
        SessionSource::Collection { collection_id } => {
            if collection_title(conn, collection_id)?.is_none() {
                return Err(AppError::Invalid("Caderno não encontrado.".into()));
            }
            let active: Option<String> = conn
                .query_row(
                    "SELECT id FROM attempts WHERE source_kind = 'collection'
                       AND source_ref = ?1 AND finished_at IS NULL",
                    params![collection_id],
                    |r| r.get(0),
                )
                .optional()?;
            if active.is_some() {
                return Err(AppError::Invalid(
                    "Este caderno já tem uma tentativa em andamento. Continue ou abandone essa \
                     tentativa antes de começar outra."
                        .into(),
                ));
            }
            let mut stmt = conn.prepare(
                "SELECT question_id FROM collection_items WHERE collection_id = ?1
                 ORDER BY position",
            )?;
            let ids = stmt
                .query_map(params![collection_id], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ("collection", Some(collection_id.clone()), ids)
        }
        SessionSource::Topic { topic_id, count } => {
            if !(1..=MAX_TOPIC_SESSION).contains(count) {
                return Err(AppError::Invalid(format!(
                    "Escolha de 1 a {MAX_TOPIC_SESSION} questões."
                )));
            }
            if topic_title(conn, *topic_id)?.is_none() {
                return Err(AppError::Invalid("Assunto não encontrado.".into()));
            }
            let ids = topic_questions(conn, *topic_id, *count)?;
            ("topic", Some(topic_id.to_string()), ids)
        }
        SessionSource::Errors => ("errors", None, crate::learning::error_questions(conn)?),
        SessionSource::Reviews => ("reviews", None, crate::learning::due_questions(conn, now)?),
    };
    if questions.is_empty() {
        return Err(AppError::Invalid(
            "Não há questões disponíveis para esta sessão.".into(),
        ));
    }
    questions.shuffle(&mut rand::rng());

    let id = new_attempt_id();
    conn.execute(
        "INSERT INTO attempts (id, source_kind, source_ref, feedback_mode, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, kind, source_ref, request.feedback_mode.as_str(), now],
    )?;
    let mut insert = conn.prepare(
        "INSERT INTO responses (attempt_id, position, question_id, snapshot)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (position, question_id) in questions.iter().enumerate() {
        let snapshot = serde_json::to_string(&snapshot(conn, question_id)?)?;
        insert.execute(params![id, position as i64, question_id, snapshot])?;
    }
    load(conn, &id)
}

/// The attempt as the frontend may see it.
pub fn load(conn: &Connection, id: &str) -> AppResult<AttemptView> {
    let header = load_header(conn, id)?;
    let mut stmt = conn.prepare(
        "SELECT r.position, r.question_id, r.snapshot, r.choice, r.eliminated, r.flagged,
                r.revealed_at IS NOT NULL, q.answer_id, q.explanation, q.refs, q.annulled,
                COALESCE(json_extract(q.provenance, '$.disputed'), 0)
         FROM responses r JOIN questions q ON q.id = r.question_id
         WHERE r.attempt_id = ?1 ORDER BY r.position",
    )?;
    let rows = stmt.query_map(params![id], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, bool>(5)?,
            r.get::<_, bool>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, String>(9)?,
            r.get::<_, bool>(10)?,
            r.get::<_, bool>(11)?,
        ))
    })?;
    let mut questions = Vec::new();
    let mut outcomes = Vec::new();
    for row in rows {
        let (
            position,
            question_id,
            snap,
            choice,
            eliminated,
            flagged,
            revealed,
            key,
            expl,
            refs,
            annulled,
            disputed,
        ) = row?;
        let snap: Snapshot = serde_json::from_str(&snap)?;
        let result = outcome(choice.as_deref(), &key, annulled);
        outcomes.push(result);
        // The only place the key, explanation and refs enter a view.
        let reveal = if revealed {
            Some(Reveal {
                answer_id: key,
                explanation: expl,
                refs: serde_json::from_str(&refs)?,
                annulled,
                outcome: result,
                disputed,
            })
        } else {
            None
        };
        questions.push(ResponseView {
            position,
            question_id,
            kind: snap.kind,
            stem: snap.stem,
            options: snap.options,
            image: snap.image,
            image_alt: snap.image_alt,
            choice,
            eliminated: serde_json::from_str(&eliminated)?,
            flagged,
            reveal,
        });
    }
    let score = header.finished_at.is_some().then(|| Score::of(outcomes));
    Ok(AttemptView {
        id: id.to_owned(),
        title: title(conn, header.source_kind, header.source_ref.as_deref())?,
        source_kind: header.source_kind,
        source_ref: header.source_ref,
        feedback_mode: header.mode,
        started_at: header.started_at,
        finished_at: header.finished_at,
        revision: header.revision,
        questions,
        score,
    })
}

/// The unfinished attempt and one of its responses, ready to change.
struct Editable {
    mode: FeedbackMode,
    options: Vec<String>,
    choice: Option<String>,
    eliminated: Vec<String>,
    revealed: bool,
}

fn editable(conn: &Connection, id: &str, position: u32) -> AppResult<Editable> {
    let header = load_header(conn, id)?;
    if header.finished_at.is_some() {
        return Err(AppError::Invalid(
            "Esta tentativa já foi finalizada e não pode mais ser alterada.".into(),
        ));
    }
    let (snap, choice, eliminated, revealed) = conn
        .query_row(
            "SELECT snapshot, choice, eliminated, revealed_at IS NOT NULL FROM responses
             WHERE attempt_id = ?1 AND position = ?2",
            params![id, position],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, bool>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::Invalid("Questão não encontrada nesta tentativa.".into()))?;
    let snap: Snapshot = serde_json::from_str(&snap)?;
    Ok(Editable {
        mode: header.mode,
        options: snap.options.into_iter().map(|o| o.id).collect(),
        choice,
        eliminated: serde_json::from_str(&eliminated)?,
        revealed,
    })
}

fn locked() -> AppError {
    AppError::Invalid("A resposta desta questão já foi revelada e não pode mudar.".into())
}

fn check_option(edit: &Editable, option_id: &str) -> AppResult<()> {
    if edit.options.iter().any(|o| o == option_id) {
        Ok(())
    } else {
        Err(AppError::Invalid("Alternativa inválida.".into()))
    }
}

fn bump(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE attempts SET revision = revision + 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

fn save(
    conn: &Connection,
    id: &str,
    position: u32,
    choice: Option<&str>,
    eliminated: &[String],
    answered_at: Option<&str>,
    reveal_at: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE responses SET choice = ?3, eliminated = ?4,
                answered_at = CASE WHEN ?3 IS NULL THEN NULL ELSE COALESCE(?5, answered_at) END,
                revealed_at = COALESCE(revealed_at, ?6)
         WHERE attempt_id = ?1 AND position = ?2",
        params![
            id,
            position,
            choice,
            serde_json::to_string(eliminated)?,
            answered_at,
            reveal_at
        ],
    )?;
    bump(conn, id)
}

/// Chooses an option, or clears the choice with `None`. In Study mode a choice reveals the
/// question and locks it. Choosing an eliminated option un-eliminates it.
pub fn answer(
    conn: &Connection,
    id: &str,
    position: u32,
    option_id: Option<&str>,
    now: &str,
) -> AppResult<AttemptView> {
    let edit = editable(conn, id, position)?;
    if edit.revealed {
        return Err(locked());
    }
    let mut eliminated = edit.eliminated.clone();
    if let Some(option) = option_id {
        check_option(&edit, option)?;
        eliminated.retain(|e| e != option);
    }
    let reveal = (edit.mode == FeedbackMode::Study && option_id.is_some()).then_some(now);
    let answered_at = option_id.map(|_| now);
    save(
        conn,
        id,
        position,
        option_id,
        &eliminated,
        answered_at,
        reveal,
    )?;
    load(conn, id)
}

/// Toggles an option's elimination. Eliminating the chosen option clears the choice.
pub fn eliminate(
    conn: &Connection,
    id: &str,
    position: u32,
    option_id: &str,
) -> AppResult<AttemptView> {
    let edit = editable(conn, id, position)?;
    if edit.revealed {
        return Err(locked());
    }
    check_option(&edit, option_id)?;
    let mut eliminated = edit.eliminated.clone();
    let mut choice = edit.choice.clone();
    if let Some(index) = eliminated.iter().position(|e| e == option_id) {
        eliminated.remove(index);
    } else {
        eliminated.push(option_id.to_owned());
        if choice.as_deref() == Some(option_id) {
            choice = None;
        }
    }
    save(
        conn,
        id,
        position,
        choice.as_deref(),
        &eliminated,
        None,
        None,
    )?;
    load(conn, id)
}

/// Sets or clears a question's flag.
pub fn flag(conn: &Connection, id: &str, position: u32, flagged: bool) -> AppResult<AttemptView> {
    editable(conn, id, position)?;
    conn.execute(
        "UPDATE responses SET flagged = ?3 WHERE attempt_id = ?1 AND position = ?2",
        params![id, position, flagged],
    )?;
    bump(conn, id)?;
    load(conn, id)
}

/// Finishes the attempt, revealing every question. `revision` must match the latest one, so
/// a stale screen can't finish over changes it hasn't seen.
pub fn finish(conn: &Connection, id: &str, revision: u32, now: &str) -> AppResult<AttemptView> {
    let header = load_header(conn, id)?;
    if header.finished_at.is_some() {
        return Err(AppError::Invalid(
            "Esta tentativa já foi finalizada.".into(),
        ));
    }
    if header.revision != revision {
        return Err(AppError::Invalid(
            "A tentativa mudou desde que esta tela foi carregada. Recarregue e tente de novo."
                .into(),
        ));
    }
    conn.execute(
        "UPDATE responses SET revealed_at = ?2 WHERE attempt_id = ?1 AND revealed_at IS NULL",
        params![id, now],
    )?;
    conn.execute(
        "UPDATE attempts SET finished_at = ?2, revision = revision + 1 WHERE id = ?1",
        params![id, now],
    )?;
    load(conn, id)
}

/// Annuls (or restores) the question at `position`, removing it from (or returning it to)
/// every score. Only allowed once that question is revealed in this attempt, so annulling
/// never gives an answer away. Applies to every attempt, since scores read current state.
pub fn set_annulled(
    conn: &Connection,
    id: &str,
    position: u32,
    annulled: bool,
    now: &str,
) -> AppResult<AttemptView> {
    load_header(conn, id)?;
    let (question_id, revealed) = conn
        .query_row(
            "SELECT question_id, revealed_at IS NOT NULL FROM responses
             WHERE attempt_id = ?1 AND position = ?2",
            params![id, position],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::Invalid("Questão não encontrada nesta tentativa.".into()))?;
    if !revealed {
        return Err(AppError::Invalid(
            "Só é possível anular uma questão depois de ver o gabarito.".into(),
        ));
    }
    conn.execute(
        "UPDATE questions SET annulled = ?2, updated_at = ?3 WHERE id = ?1",
        params![question_id, annulled, now],
    )?;
    load(conn, id)
}

/// An unfinished attempt, for the "resume" list on the home page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ActiveAttempt {
    pub id: String,
    pub title: String,
    pub source_kind: SourceKind,
    pub feedback_mode: FeedbackMode,
    pub started_at: String,
    /// Questions answered so far.
    pub answered: u32,
    pub total: u32,
}

/// Columns of the unfinished-attempts query.
type ActiveRow = (String, String, Option<String>, String, String, u32, u32);

/// Every unfinished attempt, newest first. Carries counts only, never questions or keys.
pub fn active(conn: &Connection) -> AppResult<Vec<ActiveAttempt>> {
    let rows: Vec<ActiveRow> = conn
        .prepare(
            "SELECT a.id, a.source_kind, a.source_ref, a.feedback_mode, a.started_at,
                    COALESCE(SUM(r.choice IS NOT NULL), 0), COUNT(r.position)
             FROM attempts a LEFT JOIN responses r ON r.attempt_id = a.id
             WHERE a.finished_at IS NULL
             GROUP BY a.id ORDER BY a.started_at DESC, a.id",
        )?
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    rows.into_iter()
        .map(
            |(id, kind, source_ref, mode, started_at, answered, total)| {
                let source_kind = source_kind(&kind)?;
                Ok(ActiveAttempt {
                    title: title(conn, source_kind, source_ref.as_deref())?,
                    id,
                    source_kind,
                    feedback_mode: FeedbackMode::parse(&mode)?,
                    started_at,
                    answered,
                    total,
                })
            },
        )
        .collect()
}

/// Deletes an unfinished attempt. Finished attempts are history and are kept forever.
pub fn abandon(conn: &Connection, id: &str) -> AppResult<()> {
    let header = load_header(conn, id)?;
    if header.finished_at.is_some() {
        return Err(AppError::Invalid(
            "Tentativas finalizadas fazem parte do histórico e não podem ser apagadas.".into(),
        ));
    }
    conn.execute("DELETE FROM attempts WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests;
