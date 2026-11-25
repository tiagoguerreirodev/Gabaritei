use super::*;
use crate::bank::question::tests::multiple;
use crate::bank::{TopicPath, import_questions};
use crate::db::Database;
use crate::ids::{AttemptId, CollectionId};
use rusqlite::params;

const T0: &str = "2026-11-01T10:00:00.000Z";

type Path = (&'static str, &'static str, &'static str);
type Spec<'a> = (&'a str, &'a [Path]);

/// Fixture builder: questions are `(local id, [topic paths])`; every question's key is `o0`.
struct Fixture {
    db: Database,
    ids: BTreeMap<String, QuestionId>,
}

fn fixture(questions: &[Spec]) -> Fixture {
    let db = Database::open_in_memory().unwrap();
    let inputs: Vec<_> = questions
        .iter()
        .map(|(local, topics)| {
            let mut input = multiple(local, &format!("Enunciado {local}"), 0);
            input.topics = topics
                .iter()
                .map(|(a, m, t)| TopicPath::new(a, m, t))
                .collect();
            input
        })
        .collect();
    let ids = db
        .transaction(|tx| Ok(import_questions(tx, inputs, T0)?.ids_by_local_id))
        .unwrap();
    Fixture {
        db,
        ids: ids.into_iter().collect(),
    }
}

impl Fixture {
    fn id(&self, local: &str) -> QuestionId {
        self.ids[local].clone()
    }

    /// Option ids are content hashes, so tests say `o0` for the key and `o1` for a wrong
    /// option and this finds the real ids.
    fn option(&self, local: &str, symbol: &str) -> String {
        self.db
            .with(|conn| {
                let (answer, options): (String, String) = conn.query_row(
                    "SELECT answer_id, options FROM questions WHERE id = ?1",
                    params![self.id(local)],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                if symbol == "o0" {
                    return Ok(answer);
                }
                let options: Vec<serde_json::Value> = serde_json::from_str(&options).unwrap();
                Ok(options
                    .iter()
                    .map(|o| o["id"].as_str().unwrap().to_owned())
                    .find(|id| *id != answer)
                    .unwrap())
            })
            .unwrap()
    }

    /// A collection holding the given questions, with optional tags.
    fn collection(&self, id: &str, contest: Option<&str>, board: Option<&str>, locals: &[&str]) {
        let id = CollectionId::from(id);
        self.db
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO collections (id, title, kind, contest, board, created_at)
                     VALUES (?1, ?1, 'imported', ?2, ?3, ?4)",
                    params![id, contest, board, T0],
                )?;
                for (i, local) in locals.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO collection_items (collection_id, position, question_id)
                         VALUES (?1, ?2, ?3)",
                        params![id, i as i64, self.id(local)],
                    )?;
                }
                Ok(())
            })
            .unwrap();
    }

    /// An attempt with `(local id, choice)` responses; each is answered at `finished_at`
    /// unless `answered_at` says otherwise. `finished_at: None` leaves it unfinished.
    fn attempt(
        &self,
        id: &str,
        mode: &str,
        finished_at: Option<&str>,
        responses: &[(&str, Option<&str>)],
    ) {
        self.attempt_at(
            id,
            mode,
            finished_at,
            &responses
                .iter()
                .map(|(q, c)| (*q, *c, finished_at.unwrap_or(T0)))
                .collect::<Vec<_>>(),
        );
    }

    fn attempt_at(
        &self,
        id: &str,
        mode: &str,
        finished_at: Option<&str>,
        responses: &[(&str, Option<&str>, &str)],
    ) {
        // Resolve option ids first: `option` takes the database lock itself.
        let id = AttemptId::from(id);
        let resolved: Vec<(QuestionId, Option<String>, Option<&str>)> = responses
            .iter()
            .map(|(local, choice, at)| {
                (
                    self.id(local),
                    choice.map(|c| self.option(local, c)),
                    choice.map(|_| *at),
                )
            })
            .collect();
        self.db
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO attempts (id, source_kind, feedback_mode, started_at, finished_at)
                     VALUES (?1, 'collection', ?2, ?3, ?4)",
                    params![id, mode, T0, finished_at],
                )?;
                for (i, (question_id, choice, answered_at)) in resolved.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO responses
                           (attempt_id, position, question_id, snapshot, choice, answered_at)
                         VALUES (?1, ?2, ?3, '{}', ?4, ?5)",
                        params![id, i as i64, question_id, choice, answered_at],
                    )?;
                }
                Ok(())
            })
            .unwrap();
    }

    fn query(&self, query: &PerformanceQuery) -> Performance {
        self.db.with(|conn| performance(conn, query)).unwrap()
    }

    fn all(&self) -> Performance {
        self.query(&PerformanceQuery::default())
    }

    fn set(&self, local: &str, sql: &str) {
        self.db
            .transaction(|tx| {
                tx.execute(sql, params![self.id(local)])?;
                Ok(())
            })
            .unwrap();
    }
}

const D: Path = ("Direito", "Constitucional", "Controle");
const D2: Path = ("Direito", "Constitucional", "Direitos fundamentais");
const P: Path = ("Português", "Gramática", "Crase");

#[test]
fn latest_answer_per_question_wins() {
    let f = fixture(&[("q1", &[D])]);
    f.attempt(
        "a1",
        "study",
        Some("2026-11-02T10:00:00.000Z"),
        &[("q1", Some("o1"))],
    );
    f.attempt(
        "a2",
        "study",
        Some("2026-11-03T10:00:00.000Z"),
        &[("q1", Some("o0"))],
    );
    let p = f.all();
    assert_eq!(p.overall.score.correct, 1);
    assert_eq!(p.overall.score.wrong, 0);
    assert_eq!(p.overall.score.achievement, Some(1.0));
    // The older, wrong answer still shows in the trend, which counts every answer.
    assert_eq!(p.trend.iter().map(|w| w.answered).sum::<u32>(), 2);
}

#[test]
fn the_latest_answer_is_the_one_given_last_even_when_attempts_overlap() {
    let f = fixture(&[("q1", &[D])]);
    // a1 finishes last but its answer was given first; a2 finishes first with the later answer.
    f.attempt_at(
        "a1",
        "study",
        Some("2026-11-05T10:00:00.000Z"),
        &[("q1", Some("o1"), "2026-11-02T10:00:00.000Z")],
    );
    f.attempt_at(
        "a2",
        "study",
        Some("2026-11-03T10:00:00.000Z"),
        &[("q1", Some("o0"), "2026-11-03T09:00:00.000Z")],
    );
    let p = f.all();
    assert_eq!(p.overall.score.correct, 1);
    assert_eq!(p.overall.score.wrong, 0);
}

#[test]
fn unfinished_attempts_are_ignored() {
    let f = fixture(&[("q1", &[D])]);
    f.attempt("a1", "study", None, &[("q1", Some("o0"))]);
    let p = f.all();
    assert_eq!(p.overall.score.evaluated, 0);
    assert_eq!(p.overall.available, 1);
    assert_eq!(p.overall.coverage, Some(0.0));
    assert!(p.trend.is_empty());
    assert!(p.weakest.is_empty());
}

#[test]
fn a_question_counts_once_per_group_even_with_several_topics() {
    let f = fixture(&[("q1", &[D, D2]), ("q2", &[D2]), ("q3", &[P])]);
    f.attempt(
        "a1",
        "exam",
        Some("2026-11-02T10:00:00.000Z"),
        &[("q1", Some("o0")), ("q2", Some("o1")), ("q3", Some("o0"))],
    );
    let p = f.all();
    assert_eq!(p.overall.score.evaluated, 3);
    let direito = p.areas.iter().find(|a| a.name == "Direito").unwrap();
    assert_eq!(
        direito.stats.score.evaluated, 2,
        "q1 is in two topics of one matter"
    );
    let constitucional = &direito.matters[0];
    assert_eq!(constitucional.stats.score.evaluated, 2);
    let by_name = |name: &str| {
        constitucional
            .topics
            .iter()
            .find(|t| t.name == name)
            .unwrap()
    };
    assert_eq!(by_name("Controle").stats.score.evaluated, 1);
    assert_eq!(by_name("Direitos fundamentais").stats.score.evaluated, 2);
}

#[test]
fn filters_apply_before_the_latest_answer_is_picked() {
    let f = fixture(&[("q1", &[D])]);
    // Study attempt answered wrong, then a later Simulado answered right.
    f.attempt(
        "a1",
        "study",
        Some("2026-11-02T10:00:00.000Z"),
        &[("q1", Some("o1"))],
    );
    f.attempt(
        "a2",
        "exam",
        Some("2026-11-03T10:00:00.000Z"),
        &[("q1", Some("o0"))],
    );
    let study = f.query(&PerformanceQuery {
        feedback_mode: Some(FeedbackMode::Study),
        ..Default::default()
    });
    assert_eq!(study.overall.score.wrong, 1);
    assert_eq!(study.overall.score.correct, 0);
    let exam = f.query(&PerformanceQuery {
        feedback_mode: Some(FeedbackMode::Exam),
        ..Default::default()
    });
    assert_eq!(exam.overall.score.correct, 1);
    assert_eq!(f.all().overall.score.correct, 1);
    // Trend honors the mode filter too.
    assert_eq!(study.trend.iter().map(|w| w.answered).sum::<u32>(), 1);
}

fn bank_with_tagged_collections() -> Fixture {
    let f = fixture(&[("q1", &[D]), ("q2", &[D]), ("q3", &[D])]);
    f.collection("c1", Some("TRT"), Some("Cebraspe"), &["q1"]);
    f.collection("c2", Some("INSS"), Some("FGV"), &["q2", "q1"]);
    f.collection("c3", None, None, &["q3"]);
    f.attempt(
        "a1",
        "study",
        Some("2026-11-02T10:00:00.000Z"),
        &[("q1", Some("o0")), ("q2", Some("o1")), ("q3", Some("o0"))],
    );
    f
}

#[test]
fn the_contest_filter_includes_a_question_in_each_matching_collection() {
    let f = bank_with_tagged_collections();
    let contest = |c: &str| {
        f.query(&PerformanceQuery {
            contest: Some(c.into()),
            ..Default::default()
        })
    };
    // q1 belongs to both collections, so it appears under both contests.
    assert_eq!(contest("TRT").overall.score.evaluated, 1);
    assert_eq!(contest("INSS").overall.score.evaluated, 2);
    assert_eq!(contest("INSS").overall.available, 2);
}

#[test]
fn the_board_filter_includes_matching_collections() {
    let f = bank_with_tagged_collections();
    let board = f.query(&PerformanceQuery {
        board: Some("FGV".into()),
        ..Default::default()
    });
    assert_eq!(board.overall.score.evaluated, 2);
}

#[test]
fn the_two_filters_must_match_the_same_collection() {
    let f = bank_with_tagged_collections();
    let both = f.query(&PerformanceQuery {
        contest: Some("TRT".into()),
        board: Some("FGV".into()),
        ..Default::default()
    });
    assert_eq!(
        both.overall.score.evaluated, 0,
        "q1 is in a TRT collection and in an FGV one, but never in a TRT collection by FGV"
    );
    f.collection("c4", Some("TRT"), Some("FGV"), &["q3"]);
    let both = f.query(&PerformanceQuery {
        contest: Some("TRT".into()),
        board: Some("FGV".into()),
        ..Default::default()
    });
    assert_eq!(
        both.overall.score.evaluated, 1,
        "q3 is in a TRT collection by FGV"
    );
}

#[test]
fn empty_tags_disable_filtering() {
    let f = bank_with_tagged_collections();
    // Empty strings mean "no filter".
    let none = f.query(&PerformanceQuery {
        contest: Some(String::new()),
        ..Default::default()
    });
    assert_eq!(none.overall.score.evaluated, 3);
}

#[test]
fn filter_options_list_the_collections_tags() {
    let f = bank_with_tagged_collections();
    assert_eq!(f.all().filters.contests, ["INSS", "TRT"]);
    assert_eq!(f.all().filters.boards, ["Cebraspe", "FGV"]);
}

#[test]
fn an_answer_key_correction_rescores_results() {
    let f = fixture(&[("q1", &[D])]);
    f.attempt(
        "a1",
        "study",
        Some("2026-11-02T10:00:00.000Z"),
        &[("q1", Some("o1"))],
    );
    assert_eq!(f.all().overall.score.wrong, 1);
    let other = f.option("q1", "o1");
    f.set(
        "q1",
        &format!("UPDATE questions SET answer_id = '{other}' WHERE id = ?1"),
    );
    let p = f.all();
    assert_eq!(p.overall.score.correct, 1);
    assert_eq!(p.overall.score.wrong, 0);
    assert_eq!(p.trend[0].correct, 1);
}

#[test]
fn annulment_changes_the_denominator() {
    let f = fixture(&[("q1", &[D]), ("q2", &[D])]);
    f.attempt(
        "a1",
        "study",
        Some("2026-11-02T10:00:00.000Z"),
        &[("q1", Some("o0")), ("q2", Some("o1"))],
    );
    let before = f.all();
    assert_eq!(before.overall.score.achievement, Some(0.5));
    assert_eq!(before.overall.available, 2);
    f.set("q2", "UPDATE questions SET annulled = 1 WHERE id = ?1");
    let after = f.all();
    assert_eq!(after.overall.score.evaluated, 1);
    assert_eq!(after.overall.score.annulled, 1);
    assert_eq!(after.overall.score.achievement, Some(1.0));
    assert_eq!(after.overall.available, 1);
    assert_eq!(after.overall.coverage, Some(1.0));
    assert_eq!(
        after.trend[0].answered, 1,
        "annulled answers leave the trend"
    );
}

#[test]
fn blanks_count_as_evaluated_but_not_as_answered() {
    let f = fixture(&[("q1", &[D]), ("q2", &[D]), ("q3", &[D]), ("q4", &[D])]);
    f.attempt(
        "a1",
        "exam",
        Some("2026-11-02T10:00:00.000Z"),
        &[("q1", Some("o0")), ("q2", Some("o1")), ("q3", None)],
    );
    let p = f.all();
    let s = &p.overall;
    assert_eq!((s.score.correct, s.score.wrong, s.score.blank), (1, 1, 1));
    assert_eq!(s.score.evaluated, 3);
    assert_eq!(s.score.achievement, Some(1.0 / 3.0));
    assert_eq!(s.score.accuracy, Some(0.5));
    assert_eq!(s.answered, 2);
    assert_eq!(s.available, 4);
    assert_eq!(s.coverage, Some(0.5));
}

fn performance_with_strong_weak_and_unseen_groups() -> Performance {
    let strong: Vec<String> = (0..5).map(|i| format!("s{i}")).collect();
    let mut questions: Vec<Spec> = strong.iter().map(|k| (k.as_str(), &[D][..])).collect();
    questions.push(("w1", &[P]));
    questions.push(("w2", &[P]));
    questions.push(("unseen", &[("Informática", "Redes", "TCP")]));
    let f = fixture(&questions);
    let mut responses: Vec<(&str, Option<&str>)> =
        strong.iter().map(|k| (k.as_str(), Some("o0"))).collect();
    responses.push(("w1", Some("o1")));
    responses.push(("w2", Some("o0")));
    f.attempt("a1", "study", Some("2026-11-02T10:00:00.000Z"), &responses);
    f.all()
}

#[test]
fn small_samples_are_flagged() {
    let p = performance_with_strong_weak_and_unseen_groups();
    assert!(p.areas[0].stats.small_sample, "2 answers is under 5");
    assert!(!p.areas[1].stats.small_sample, "5 answers is enough");
}

#[test]
fn areas_are_sorted_from_weakest_to_unseen() {
    let p = performance_with_strong_weak_and_unseen_groups();
    let names: Vec<&str> = p.areas.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        ["Português", "Direito", "Informática"],
        "weakest first, unseen last"
    );
}

#[test]
fn the_weakest_topics_list_excludes_unseen_topics() {
    let p = performance_with_strong_weak_and_unseen_groups();
    assert_eq!(p.weakest.len(), 2, "topics never answered are not weak");
    assert_eq!(p.weakest[0].topic, "Crase");
    assert_eq!(p.weakest[1].topic, "Controle");
}

#[test]
fn weekly_trend_uses_iso_weeks_in_local_time() {
    let f = fixture(&[("q1", &[D])]);
    // Sunday 22 Nov 23:30 in Brasília is Monday 23 Nov 02:30 UTC; Monday 16 Nov is a week start.
    let answers = [
        ("q1", Some("o0"), "2026-11-16T03:00:00.000Z"), // Mon 00:00 local → week of 16
        ("q1", Some("o1"), "2026-11-16T02:59:59.000Z"), // Sun 23:59 local → week of 9
        ("q1", Some("o0"), "2026-11-23T02:30:00.000Z"), // Sun 23:30 local → week of 16
        ("q1", Some("o0"), "2026-11-23T03:00:00.000Z"), // Mon 00:00 local → week of 23
    ];
    f.attempt_at("a1", "study", Some("2026-11-24T10:00:00.000Z"), &answers);
    let local = f.query(&PerformanceQuery {
        utc_offset_minutes: -180,
        ..Default::default()
    });
    let weeks: Vec<(&str, u32, u32)> = local
        .trend
        .iter()
        .map(|w| (w.week_start.as_str(), w.answered, w.correct))
        .collect();
    assert_eq!(
        weeks,
        [
            ("2026-11-09", 1, 0),
            ("2026-11-16", 2, 2),
            ("2026-11-23", 1, 1)
        ]
    );
    assert_eq!(local.trend[1].accuracy, Some(1.0));
    let utc = f.query(&PerformanceQuery::default());
    let weeks: Vec<(&str, u32)> = utc
        .trend
        .iter()
        .map(|w| (w.week_start.as_str(), w.answered))
        .collect();
    assert_eq!(
        weeks,
        [("2026-11-16", 2), ("2026-11-23", 2)],
        "without the offset, the same answers fall into different weeks"
    );
}

#[test]
fn weeks_cross_the_year_boundary() {
    let f = fixture(&[("q1", &[D])]);
    // Thursday 1 Jan 2026 belongs to the ISO week starting Monday 29 Dec 2025.
    f.attempt_at(
        "a1",
        "study",
        Some("2026-01-05T10:00:00.000Z"),
        &[("q1", Some("o0"), "2026-01-01T12:00:00.000Z")],
    );
    assert_eq!(f.all().trend[0].week_start, "2025-12-29");
}

fn export_with_special_characters() -> String {
    let f = fixture(&[("q1", &[("=HYPERLINK(\"x\")", "Mat;éria", "-Assunto")])]);
    f.attempt(
        "a1",
        "study",
        Some("2026-11-02T10:00:00.000Z"),
        &[("q1", Some("o0"))],
    );
    f.db.with(|c| export_csv(c, &PerformanceQuery::default()))
        .unwrap()
}

#[test]
fn csv_uses_semicolons_with_a_bom() {
    let csv = export_with_special_characters();
    assert!(csv.starts_with('\u{feff}'));
    let lines: Vec<&str> = csv.trim_end().split("\r\n").collect();
    assert_eq!(lines.len(), 5, "header, overall, area, matter, topic");
    assert!(lines[0].starts_with("\u{feff}Nível;Área;Matéria;Assunto;"));
    assert_eq!(lines[1], "Geral;;;;1;0;0;0;1;100,0;100,0;1;100,0");
}

#[test]
fn csv_escapes_topic_path_cells() {
    let csv = export_with_special_characters();
    let lines: Vec<&str> = csv.trim_end().split("\r\n").collect();
    assert!(
        lines[2].starts_with("Área;\"'=HYPERLINK(\"\"x\"\")\";"),
        "guarded with an apostrophe, then quoted because it contains quotes: {}",
        lines[2]
    );
    assert!(lines[3].contains("\"Mat;éria\""));
    assert!(lines[4].contains(";'-Assunto;"));
}

#[test]
fn cells_guard_formula_prefixes() {
    assert_eq!(cell("normal"), "normal");
    assert_eq!(cell("=1+1"), "'=1+1");
    assert_eq!(cell("+55"), "'+55");
    assert_eq!(cell("@x"), "'@x");
    assert_eq!(cell("\tx"), "'\tx");
}

#[test]
fn cells_quote_csv_delimiters() {
    assert_eq!(cell("a;b"), "\"a;b\"");
    assert_eq!(cell("a\"b"), "\"a\"\"b\"");
}

#[test]
fn cells_guard_formulas_before_quoting() {
    assert_eq!(cell("=a;b"), "\"'=a;b\"");
}

#[test]
fn an_empty_bank_has_no_metrics() {
    let p = fixture(&[]).all();
    assert_eq!(p.overall.score.evaluated, 0);
    assert_eq!(p.overall.coverage, None);
    assert!(p.areas.is_empty() && p.weakest.is_empty() && p.trend.is_empty());
}

/// `cargo test --manifest-path src-tauri/Cargo.toml metrics_timing -- --ignored --nocapture`
#[test]
#[ignore = "timing run for 10k questions and 50k responses"]
fn metrics_timing() {
    let db = Database::open_in_memory().unwrap();
    let inputs: Vec<_> = (0..10_000)
        .map(|i| {
            let mut q = multiple(&format!("q{i}"), &format!("Enunciado {i}"), 0);
            q.topics = vec![TopicPath::new(
                &format!("Área {}", i % 8),
                &format!("Matéria {}", i % 40),
                &format!("Assunto {}", i % 400),
            )];
            q
        })
        .collect();
    let ids = db
        .transaction(|tx| {
            let map = import_questions(tx, inputs, T0)?.ids_by_local_id;
            for a in 0..50 {
                tx.execute(
                    "INSERT INTO attempts (id, source_kind, feedback_mode, started_at, finished_at)
                     VALUES (?1, 'collection', 'study', ?2, ?3)",
                    params![
                        format!("a{a}"),
                        T0,
                        format!("2026-{:02}-{:02}T10:00:00.000Z", 1 + a / 28, 1 + a % 28)
                    ],
                )?;
                for p in 0..1000 {
                    let q = &map[&format!("q{}", (a * 1000 + p) % 10_000)];
                    tx.execute(
                        "INSERT INTO responses (attempt_id, position, question_id, snapshot, choice, answered_at)
                         VALUES (?1, ?2, ?3, '{}', ?4, ?5)",
                        params![
                            format!("a{a}"),
                            p,
                            q,
                            if p % 3 == 0 { "o1" } else { "o0" },
                            format!("2026-{:02}-{:02}T10:00:00.000Z", 1 + a / 28, 1 + a % 28)
                        ],
                    )?;
                }
            }
            Ok(map.len())
        })
        .unwrap();
    let started = std::time::Instant::now();
    let p = db
        .with(|c| performance(c, &PerformanceQuery::default()))
        .unwrap();
    println!(
        "performance() over {ids} questions and 50000 responses: {:?} ({} areas, {} weeks)",
        started.elapsed(),
        p.areas.len(),
        p.trend.len()
    );
}

#[test]
fn topic_accuracy_feeds_the_bank_tree() {
    let f = fixture(&[("q1", &[D]), ("q2", &[D]), ("q3", &[P])]);
    f.attempt(
        "a1",
        "study",
        Some("2026-11-02T10:00:00.000Z"),
        &[("q1", Some("o0")), ("q2", Some("o1"))],
    );
    let tree = f.db.with(|c| crate::bank::catalog::topic_tree(c)).unwrap();
    let accuracy = |name: &str| {
        tree.areas
            .iter()
            .flat_map(|a| &a.matters)
            .flat_map(|m| &m.topics)
            .find(|t| t.name == name)
            .unwrap()
            .accuracy
    };
    assert_eq!(accuracy("Controle"), Some(0.5));
    assert_eq!(accuracy("Crase"), None, "never answered");
}

#[test]
fn a_blank_in_a_newer_attempt_does_not_hide_an_earlier_answer() {
    let f = fixture(&[("q1", &[D]), ("q2", &[D])]);
    f.attempt(
        "a1",
        "exam",
        Some("2026-11-02T10:00:00.000Z"),
        &[("q1", Some("o0")), ("q2", None)],
    );
    // A later Simulado leaves both blank.
    f.attempt(
        "a2",
        "exam",
        Some("2026-11-03T10:00:00.000Z"),
        &[("q1", None), ("q2", None)],
    );
    let s = f.all().overall;
    assert_eq!(s.score.correct, 1, "q1 keeps its earlier correct answer");
    assert_eq!(
        s.score.blank, 1,
        "q2 was never answered, so it counts as blank"
    );
    assert_eq!(s.answered, 1);
    assert_eq!(s.coverage, Some(0.5), "coverage means ever answered");
}

#[test]
fn tags_are_compared_without_surrounding_whitespace() {
    let f = fixture(&[("q1", &[D])]);
    f.collection("c1", Some("TRT "), Some(" FGV"), &["q1"]);
    f.attempt(
        "a1",
        "study",
        Some("2026-11-02T10:00:00.000Z"),
        &[("q1", Some("o0"))],
    );
    assert_eq!(f.all().filters.contests, ["TRT"]);
    assert_eq!(f.all().filters.boards, ["FGV"]);
    let p = f.query(&PerformanceQuery {
        contest: Some("TRT".into()),
        board: Some("FGV".into()),
        ..Default::default()
    });
    assert_eq!(p.overall.score.evaluated, 1);
}

#[test]
fn absurd_time_zone_offsets_are_clamped() {
    let f = fixture(&[("q1", &[D])]);
    f.attempt(
        "a1",
        "study",
        Some("2026-11-02T10:00:00.000Z"),
        &[("q1", Some("o0"))],
    );
    let week = |offset: i32| {
        f.query(&PerformanceQuery {
            utc_offset_minutes: offset,
            ..Default::default()
        })
        .trend[0]
            .week_start
            .clone()
    };
    // 10:00 UTC on Monday 2 Nov: UTC+14 is Tuesday 3 Nov (same week), UTC-12 is Sunday 1 Nov.
    assert_eq!(week(i32::MAX), "2026-11-02", "clamped to UTC+14");
    assert_eq!(week(14 * 60), "2026-11-02");
    assert_eq!(week(i32::MIN), "2026-10-26", "clamped to UTC-12");
    assert_eq!(week(-12 * 60), "2026-10-26");
}

#[test]
fn home_counts_as_held_only_the_due_reviews_that_sit_in_an_open_attempt() {
    let f = fixture(&[("q1", &[D]), ("q2", &[D]), ("q3", &[D]), ("q4", &[D])]);
    // q1: open attempt. q2: only a finished attempt. q3: no attempt. q4: open attempt, not due.
    f.attempt(
        "open",
        "study",
        None,
        &[("q1", Some("o0")), ("q4", Some("o0"))],
    );
    f.attempt(
        "done",
        "study",
        Some("2026-11-02T10:00:00.000Z"),
        &[("q2", Some("o0"))],
    );
    f.db.transaction(|tx| {
        for (local, due) in [
            ("q1", "2026-11-10T00:00:00.000Z"),
            ("q2", "2026-11-10T00:00:00.000Z"),
            ("q3", "2026-11-10T00:00:00.000Z"),
            ("q4", "2026-12-10T00:00:00.000Z"),
        ] {
            tx.execute(
                "INSERT INTO review_state (question_id, due_at, state) VALUES (?1, ?2, '{}')",
                params![f.id(local), due],
            )?;
        }
        Ok(())
    })
    .unwrap();

    let home = f.db.with(|c| home(c, "2026-11-20T00:00:00.000Z")).unwrap();

    assert_eq!(home.due_reviews, 3, "q1, q2 and q3 are due; q4 is not");
    assert_eq!(
        home.due_in_open_attempt, 1,
        "only q1 is due and held by an open attempt"
    );
}

fn home_with_bank_activity() -> HomeSummary {
    let f = fixture(&[("q1", &[D]), ("q2", &[P])]);
    f.collection("c1", None, None, &["q1", "q2"]);
    f.attempt(
        "done",
        "study",
        Some("2026-11-02T10:00:00.000Z"),
        &[("q1", Some("o0"))],
    );
    f.db.transaction(|tx| {
        tx.execute(
            "INSERT INTO attempts (id, source_kind, source_ref, feedback_mode, started_at)
             VALUES ('open', 'collection', 'c1', 'exam', '2026-11-05T09:00:00.000Z')",
            [],
        )?;
        for (i, local) in ["q1", "q2"].iter().enumerate() {
            tx.execute(
                "INSERT INTO responses (attempt_id, position, question_id, snapshot, choice)
                 VALUES ('open', ?1, ?2, '{}', ?3)",
                params![i as i64, f.id(local), (i == 0).then_some("x")],
            )?;
        }
        tx.execute(
            "INSERT INTO review_state (question_id, due_at, state) VALUES
               (?1, '2026-11-10T00:00:00.000Z', '{}'), (?2, '2026-12-10T00:00:00.000Z', '{}')",
            params![f.id("q1"), f.id("q2")],
        )?;
        tx.execute(
            "INSERT INTO drafts (id, created_at, status, request, package) VALUES
               ('d1', ?1, 'review', '{}', '{}'), ('d2', ?1, 'discarded', '{}', '{}')",
            params![T0],
        )?;
        Ok(())
    })
    .unwrap();
    f.db.with(|c| home(c, "2026-11-20T00:00:00.000Z")).unwrap()
}

#[test]
fn home_summarizes_the_bank() {
    let home = home_with_bank_activity();
    assert_eq!(
        (home.questions, home.overall.score.correct, home.areas.len()),
        (2, 1, 2)
    );
}

#[test]
fn home_summarizes_due_reviews() {
    let home = home_with_bank_activity();
    assert_eq!((home.due_reviews, home.due_in_open_attempt), (1, 1));
}

#[test]
fn home_counts_drafts_waiting_for_review() {
    assert_eq!(home_with_bank_activity().drafts_waiting, 1);
}

#[test]
fn home_reports_active_attempts() {
    let home = home_with_bank_activity();
    assert_eq!(home.active_attempts.len(), 1);
    let open = &home.active_attempts[0];
    assert_eq!(
        (open.id.as_str(), open.answered, open.total),
        ("open", 1, 2)
    );
    assert_eq!(open.title, "c1", "falls back to the collection's title");
}
