//! Read-only views for browsing the bank: the topic tree and collections (spec §3.1, Banco).
//!
//! These views never include answer keys or explanations (AGENTS.md rule 1).

use std::cmp::Ordering;
use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::error::{AppError, AppResult};

/// Default and maximum page sizes for the collections list.
pub const DEFAULT_PAGE_SIZE: u32 = 20;
pub const MAX_PAGE_SIZE: u32 = 100;

/// Lowercase, accent-free text for search and Portuguese alphabetical order.
pub fn fold(text: &str) -> String {
    text.nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
}

/// Portuguese-friendly order: accents and case only break ties.
fn cmp_pt(a: &str, b: &str) -> Ordering {
    fold(a).cmp(&fold(b)).then_with(|| a.cmp(b))
}

// ---- Topic tree ------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TopicTree {
    pub areas: Vec<AreaNode>,
    /// Questions in the bank (each counted once).
    pub questions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AreaNode {
    pub name: String,
    /// Distinct questions under this area.
    pub questions: u32,
    pub matters: Vec<MatterNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MatterNode {
    pub name: String,
    /// Distinct questions under this matter.
    pub questions: u32,
    pub topics: Vec<TopicNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TopicNode {
    pub id: i64,
    pub name: String,
    pub questions: u32,
}

/// Area › matter › topic with distinct question counts at every level. A question filed
/// under two topics of the same matter counts once for that matter.
pub fn topic_tree(conn: &Connection) -> AppResult<TopicTree> {
    let mut area_counts: BTreeMap<String, u32> = BTreeMap::new();
    for row in conn
        .prepare(
            "SELECT t.area, COUNT(DISTINCT qt.question_id)
             FROM topics t JOIN question_topics qt ON qt.topic_id = t.id GROUP BY t.area",
        )?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))?
    {
        let (area, count) = row?;
        area_counts.insert(area, count);
    }
    let mut matter_counts: BTreeMap<(String, String), u32> = BTreeMap::new();
    for row in conn
        .prepare(
            "SELECT t.area, t.matter, COUNT(DISTINCT qt.question_id)
             FROM topics t JOIN question_topics qt ON qt.topic_id = t.id
             GROUP BY t.area, t.matter",
        )?
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u32>(2)?,
            ))
        })?
    {
        let (area, matter, count) = row?;
        matter_counts.insert((area, matter), count);
    }

    let mut areas: BTreeMap<String, BTreeMap<String, Vec<TopicNode>>> = BTreeMap::new();
    for row in conn
        .prepare(
            "SELECT t.id, t.area, t.matter, t.topic, COUNT(qt.question_id)
             FROM topics t JOIN question_topics qt ON qt.topic_id = t.id
             GROUP BY t.id",
        )?
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, u32>(4)?,
            ))
        })?
    {
        let (id, area, matter, topic, count) = row?;
        areas
            .entry(area)
            .or_default()
            .entry(matter)
            .or_default()
            .push(TopicNode {
                id,
                name: topic,
                questions: count,
            });
    }

    let mut tree: Vec<AreaNode> = areas
        .into_iter()
        .map(|(area, matters)| {
            let mut matters: Vec<MatterNode> = matters
                .into_iter()
                .map(|(matter, mut topics)| {
                    topics.sort_by(|a, b| cmp_pt(&a.name, &b.name));
                    MatterNode {
                        questions: matter_counts[&(area.clone(), matter.clone())],
                        name: matter,
                        topics,
                    }
                })
                .collect();
            matters.sort_by(|a, b| cmp_pt(&a.name, &b.name));
            AreaNode {
                questions: area_counts[&area],
                name: area,
                matters,
            }
        })
        .collect();
    tree.sort_by(|a, b| cmp_pt(&a.name, &b.name));

    let questions = conn.query_row("SELECT COUNT(*) FROM questions", [], |r| r.get(0))?;
    Ok(TopicTree {
        areas: tree,
        questions,
    })
}

// ---- Collections -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum CollectionKind {
    Imported,
    Generated,
    Practice,
}

impl CollectionKind {
    fn parse(value: &str) -> AppResult<Self> {
        match value {
            "imported" => Ok(Self::Imported),
            "generated" => Ok(Self::Generated),
            "practice" => Ok(Self::Practice),
            other => Err(AppError::Internal(format!(
                "unknown collection kind {other:?}"
            ))),
        }
    }
}

/// Filters and page for the collections list. Empty strings mean "no filter".
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct CollectionQuery {
    /// Matches the title, ignoring case and accents.
    pub search: String,
    pub contest: String,
    pub board: String,
    /// Zero-based.
    pub page: u32,
    /// 1–100; 0 means the default (20).
    pub page_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CollectionSummary {
    pub id: String,
    pub title: String,
    pub kind: CollectionKind,
    pub contest: Option<String>,
    pub board: Option<String>,
    pub questions: u32,
    pub created_at: String,
    /// When the latest attempt finished, if any.
    pub last_finished_at: Option<String>,
    /// Whether an attempt is in progress.
    pub in_progress: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CollectionPage {
    pub items: Vec<CollectionSummary>,
    /// Collections matching the filters, across all pages.
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
    /// Every contest and board used by some collection, for the filter menus.
    pub contests: Vec<String>,
    pub boards: Vec<String>,
}

fn all_collections(conn: &Connection) -> AppResult<Vec<CollectionSummary>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.title, c.kind, c.contest, c.board, c.created_at,
                (SELECT COUNT(*) FROM collection_items ci WHERE ci.collection_id = c.id),
                (SELECT MAX(a.finished_at) FROM attempts a
                  WHERE a.source_kind = 'collection' AND a.source_ref = c.id),
                EXISTS (SELECT 1 FROM attempts a
                  WHERE a.source_kind = 'collection' AND a.source_ref = c.id
                    AND a.finished_at IS NULL)
         FROM collections c",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, u32>(6)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, bool>(8)?,
        ))
    })?;
    rows.map(|row| {
        let (id, title, kind, contest, board, created_at, questions, last_finished_at, in_progress) =
            row?;
        Ok(CollectionSummary {
            id,
            title,
            kind: CollectionKind::parse(&kind)?,
            contest,
            board,
            questions,
            created_at,
            last_finished_at,
            in_progress,
        })
    })
    .collect()
}

fn distinct(values: impl Iterator<Item = Option<String>>) -> Vec<String> {
    let mut values: Vec<String> = values.flatten().collect();
    values.sort_by(|a, b| cmp_pt(a, b));
    values.dedup();
    values
}

/// One page of collections matching the query, ordered by contest and then title.
pub fn collections(conn: &Connection, query: &CollectionQuery) -> AppResult<CollectionPage> {
    let page_size = match query.page_size {
        0 => DEFAULT_PAGE_SIZE,
        n => n.min(MAX_PAGE_SIZE),
    };
    let all = all_collections(conn)?;
    let contests = distinct(all.iter().map(|c| c.contest.clone()));
    let boards = distinct(all.iter().map(|c| c.board.clone()));

    let search = fold(query.search.trim());
    let mut matching: Vec<CollectionSummary> = all
        .into_iter()
        .filter(|c| search.is_empty() || fold(&c.title).contains(&search))
        .filter(|c| query.contest.is_empty() || c.contest.as_deref() == Some(&query.contest))
        .filter(|c| query.board.is_empty() || c.board.as_deref() == Some(&query.board))
        .collect();
    matching.sort_by(|a, b| {
        cmp_pt(
            a.contest.as_deref().unwrap_or(""),
            b.contest.as_deref().unwrap_or(""),
        )
        .then_with(|| cmp_pt(&a.title, &b.title))
        .then_with(|| a.id.cmp(&b.id))
    });

    let total = matching.len() as u32;
    let items = matching
        .into_iter()
        .skip((query.page * page_size) as usize)
        .take(page_size as usize)
        .collect();
    Ok(CollectionPage {
        items,
        total,
        page: query.page,
        page_size,
        contests,
        boards,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum FeedbackMode {
    Study,
    Exam,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AttemptListItem {
    pub id: String,
    pub feedback_mode: FeedbackMode,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// Questions answered so far.
    pub answered: u32,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CollectionDetail {
    pub id: String,
    pub title: String,
    pub kind: CollectionKind,
    pub contest: Option<String>,
    pub board: Option<String>,
    pub questions: u32,
    pub created_at: String,
    /// Newest first.
    pub attempts: Vec<AttemptListItem>,
}

/// One collection with its attempts.
pub fn collection(conn: &Connection, id: &str) -> AppResult<CollectionDetail> {
    let row = conn
        .query_row(
            "SELECT c.title, c.kind, c.contest, c.board, c.created_at,
                    (SELECT COUNT(*) FROM collection_items ci WHERE ci.collection_id = c.id)
             FROM collections c WHERE c.id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, u32>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((title, kind, contest, board, created_at, questions)) = row else {
        return Err(AppError::Invalid("Caderno não encontrado.".into()));
    };
    let mut stmt = conn.prepare(
        "SELECT a.id, a.feedback_mode, a.started_at, a.finished_at,
                (SELECT COUNT(*) FROM responses r WHERE r.attempt_id = a.id
                   AND r.answered_at IS NOT NULL),
                (SELECT COUNT(*) FROM responses r WHERE r.attempt_id = a.id)
         FROM attempts a
         WHERE a.source_kind = 'collection' AND a.source_ref = ?1
         ORDER BY a.started_at DESC, a.id DESC",
    )?;
    let attempts = stmt
        .query_map(params![id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, u32>(4)?,
                r.get::<_, u32>(5)?,
            ))
        })?
        .map(|row| {
            let (id, mode, started_at, finished_at, answered, total) = row?;
            let feedback_mode = match mode.as_str() {
                "study" => FeedbackMode::Study,
                "exam" => FeedbackMode::Exam,
                other => {
                    return Err(AppError::Internal(format!(
                        "unknown feedback mode {other:?}"
                    )));
                }
            };
            Ok(AttemptListItem {
                id,
                feedback_mode,
                started_at,
                finished_at,
                answered,
                total,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(CollectionDetail {
        id: id.to_owned(),
        title,
        kind: CollectionKind::parse(&kind)?,
        contest,
        board,
        questions,
        created_at,
        attempts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank::question::tests::multiple;
    use crate::bank::{TopicPath, import_questions};
    use crate::db::Database;

    const NOW: &str = "2026-11-19T12:00:00.000Z";

    /// Three questions: q1 in two topics of the same matter, q2 and q3 elsewhere.
    fn bank() -> (Database, Vec<String>) {
        let db = Database::open_in_memory().unwrap();
        let mut q1 = multiple("q1", "Um", 0);
        q1.topics = vec![
            TopicPath::new("Direito", "Constitucional", "Controle"),
            TopicPath::new("Direito", "Constitucional", "Ação direta"),
        ];
        let mut q2 = multiple("q2", "Dois", 0);
        q2.topics = vec![TopicPath::new("Direito", "Administrativo", "Servidores")];
        let mut q3 = multiple("q3", "Três", 0);
        q3.topics = vec![TopicPath::new("Informática", "Banco de dados", "SQL")];
        let ids = db
            .transaction(|tx| {
                let outcome = import_questions(tx, vec![q1, q2, q3], NOW)?;
                Ok(["q1", "q2", "q3"]
                    .map(|l| outcome.ids_by_local_id[l].clone())
                    .to_vec())
            })
            .unwrap();
        (db, ids)
    }

    fn add_collection(
        db: &Database,
        id: &str,
        title: &str,
        contest: Option<&str>,
        board: Option<&str>,
        questions: &[String],
    ) {
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO collections (id, title, kind, contest, board, created_at)
                 VALUES (?1, ?2, 'imported', ?3, ?4, ?5)",
                params![id, title, contest, board, NOW],
            )?;
            for (i, q) in questions.iter().enumerate() {
                tx.execute(
                    "INSERT INTO collection_items (collection_id, position, question_id)
                     VALUES (?1, ?2, ?3)",
                    params![id, i as i64, q],
                )?;
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn fold_removes_case_and_accents() {
        assert_eq!(
            fold("Administração Pública ÇÃO"),
            "administracao publica cao"
        );
    }

    #[test]
    fn tree_counts_distinct_questions_at_every_level() {
        let (db, _) = bank();
        let tree = db.with(|c| topic_tree(c)).unwrap();
        assert_eq!(tree.questions, 3);
        let names: Vec<&str> = tree.areas.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Direito", "Informática"]);

        let direito = &tree.areas[0];
        assert_eq!(direito.questions, 2);
        let constitucional = &direito.matters[1];
        assert_eq!(constitucional.name, "Constitucional");
        assert_eq!(
            constitucional.questions, 1,
            "q1 is in two topics but counts once"
        );
        let topics: Vec<(&str, u32)> = constitucional
            .topics
            .iter()
            .map(|t| (t.name.as_str(), t.questions))
            .collect();
        assert_eq!(
            topics,
            [("Ação direta", 1), ("Controle", 1)],
            "accent-aware order"
        );
    }

    #[test]
    fn empty_bank_has_an_empty_tree() {
        let db = Database::open_in_memory().unwrap();
        let tree = db.with(|c| topic_tree(c)).unwrap();
        assert_eq!(
            tree,
            TopicTree {
                areas: vec![],
                questions: 0
            }
        );
    }

    fn sample_collections() -> Database {
        let (db, ids) = bank();
        add_collection(
            &db,
            "c1",
            "Direito › Administração",
            Some("ALEPA"),
            None,
            &ids[..2],
        );
        add_collection(
            &db,
            "c2",
            "Informática › SQL",
            Some("Perito"),
            Some("FGV"),
            &ids[2..],
        );
        add_collection(
            &db,
            "c3",
            "Direito › Ação direta",
            Some("ALEPA"),
            Some("Cebraspe"),
            &ids[..1],
        );
        db
    }

    fn titles(page: &CollectionPage) -> Vec<&str> {
        page.items.iter().map(|c| c.title.as_str()).collect()
    }

    #[test]
    fn collections_are_ordered_by_contest_then_title_with_filter_options() {
        let db = sample_collections();
        let page = db
            .with(|c| collections(c, &CollectionQuery::default()))
            .unwrap();
        assert_eq!(
            titles(&page),
            [
                "Direito › Ação direta",
                "Direito › Administração",
                "Informática › SQL"
            ]
        );
        assert_eq!(page.total, 3);
        assert_eq!(page.page_size, DEFAULT_PAGE_SIZE);
        assert_eq!(page.contests, ["ALEPA", "Perito"]);
        assert_eq!(page.boards, ["Cebraspe", "FGV"]);
        assert_eq!(page.items[1].questions, 2);
        assert!(!page.items[1].in_progress);
    }

    #[test]
    fn search_ignores_case_and_accents() {
        let db = sample_collections();
        let query = |search: &str| CollectionQuery {
            search: search.into(),
            ..Default::default()
        };
        let page = db
            .with(|c| collections(c, &query("  ADMINISTRACAO ")))
            .unwrap();
        assert_eq!(titles(&page), ["Direito › Administração"]);
        // "ação" folds to "acao", which also occurs inside "administração".
        let page = db.with(|c| collections(c, &query("ação"))).unwrap();
        assert_eq!(page.total, 2);
        let page = db.with(|c| collections(c, &query("ACAO DIRETA"))).unwrap();
        assert_eq!(titles(&page), ["Direito › Ação direta"]);
        let page = db.with(|c| collections(c, &query("nada disso"))).unwrap();
        assert_eq!(page.total, 0);
    }

    #[test]
    fn contest_and_board_filters_combine() {
        let db = sample_collections();
        let page = db
            .with(|c| {
                collections(
                    c,
                    &CollectionQuery {
                        contest: "ALEPA".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(
            titles(&page),
            ["Direito › Ação direta", "Direito › Administração"]
        );
        let page = db
            .with(|c| {
                collections(
                    c,
                    &CollectionQuery {
                        contest: "ALEPA".into(),
                        board: "Cebraspe".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(titles(&page), ["Direito › Ação direta"]);
        assert_eq!(
            page.contests,
            ["ALEPA", "Perito"],
            "options ignore the filters"
        );
    }

    #[test]
    fn pagination_slices_and_reports_the_total() {
        let db = sample_collections();
        let page = |n: u32| {
            db.with(|c| {
                collections(
                    c,
                    &CollectionQuery {
                        page: n,
                        page_size: 2,
                        ..Default::default()
                    },
                )
            })
            .unwrap()
        };
        assert_eq!(titles(&page(0)).len(), 2);
        assert_eq!(titles(&page(1)), ["Informática › SQL"]);
        assert!(page(2).items.is_empty());
        assert_eq!(page(1).total, 3);
        let huge = db
            .with(|c| {
                collections(
                    c,
                    &CollectionQuery {
                        page_size: 5000,
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(huge.page_size, MAX_PAGE_SIZE);
    }

    #[test]
    fn collection_detail_lists_attempts_newest_first() {
        let db = sample_collections();
        db.with(|c| {
            c.execute_batch(
                "INSERT INTO attempts (id, source_kind, source_ref, feedback_mode, started_at, finished_at)
                 VALUES ('a1', 'collection', 'c1', 'exam', '2026-11-01T10:00:00.000Z', '2026-11-01T11:00:00.000Z'),
                        ('a2', 'collection', 'c1', 'study', '2026-11-02T10:00:00.000Z', NULL),
                        ('a3', 'collection', 'c2', 'study', '2026-11-03T10:00:00.000Z', NULL);",
            )?;
            Ok(())
        })
        .unwrap();

        let detail = db.with(|c| collection(c, "c1")).unwrap();
        assert_eq!(detail.questions, 2);
        assert_eq!(detail.contest.as_deref(), Some("ALEPA"));
        let attempts: Vec<(&str, FeedbackMode, bool)> = detail
            .attempts
            .iter()
            .map(|a| (a.id.as_str(), a.feedback_mode, a.finished_at.is_some()))
            .collect();
        assert_eq!(
            attempts,
            [
                ("a2", FeedbackMode::Study, false),
                ("a1", FeedbackMode::Exam, true)
            ]
        );

        let page = db
            .with(|c| collections(c, &CollectionQuery::default()))
            .unwrap();
        let c1 = page.items.iter().find(|c| c.id == "c1").unwrap();
        assert!(c1.in_progress);
        assert_eq!(
            c1.last_finished_at.as_deref(),
            Some("2026-11-01T11:00:00.000Z")
        );
    }

    /// Times the Banco queries on the real legacy bank. Run with
    /// `LEGACY_CLONE=/path/to/Simulados-Concursos cargo test --release real_bank_browsing -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a clone of the legacy repository"]
    fn real_bank_browsing_is_fast() {
        let clone = std::env::var("LEGACY_CLONE").expect("set LEGACY_CLONE");
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("gabaritei.sqlite")).unwrap();
        crate::migrate::import_legacy_bank(
            &db,
            &dir.path().join("images"),
            std::path::Path::new(&clone),
            NOW,
        )
        .unwrap();
        let time = |label: &str, f: &dyn Fn() -> usize| {
            let started = std::time::Instant::now();
            let n = f();
            println!("{label}: {n} items in {:?}", started.elapsed());
        };
        time("topic_tree areas", &|| {
            db.with(|c| topic_tree(c)).unwrap().areas.len()
        });
        time("collections page 0", &|| {
            db.with(|c| collections(c, &CollectionQuery::default()))
                .unwrap()
                .total as usize
        });
        time("collections search", &|| {
            let query = CollectionQuery {
                search: "informatica".into(),
                ..Default::default()
            };
            db.with(|c| collections(c, &query)).unwrap().total as usize
        });
        let first = db
            .with(|c| collections(c, &CollectionQuery::default()))
            .unwrap()
            .items[0]
            .id
            .clone();
        time("collection detail", &|| {
            db.with(|c| collection(c, &first)).unwrap().questions as usize
        });
    }

    #[test]
    fn unknown_collection_is_a_pt_br_error() {
        let db = Database::open_in_memory().unwrap();
        let error = db.with(|c| collection(c, "nope")).unwrap_err();
        assert_eq!(error.to_string(), "Caderno não encontrado.");
    }
}
