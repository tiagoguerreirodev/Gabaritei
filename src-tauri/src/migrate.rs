//! One-time import of the legacy Caderno Local question bank (spec §3.6 "First-run migration").
//!
//! Reads `biblioteca/` from a clone of the legacy repository and verifies everything before
//! writing anything: the manifest, the size and SHA-256 of `estado.json.gz`, the snapshot
//! format, references between tables, every image hash, every question against the bank
//! rules, and that each question keeps its legacy ID (the content fingerprint). Then it
//! copies the images and imports questions, topics and collections in one transaction.
//!
//! Attempts, responses, drafts, exposure, review state, syllabus, aliases, imports, versions
//! and settings are not imported. Collections keep their contest; the legacy board value
//! ("Acervo autoral") isn't an exam board, so migrated collections get none.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bank::identity::sha256_hex;
use crate::bank::question::is_valid_image_name;
use crate::bank::{
    Provenance, QuestionInput, QuestionOption, QuestionType, TopicPath, apply_import, plan_import,
};
use crate::db::{Database, get_setting, set_setting};
use crate::error::{AppError, AppResult};

/// Settings key recording that the migration ran.
pub const STATUS_KEY: &str = "legacyImport";

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_STATE_BYTES: u64 = 80 * 1024 * 1024;
const MAX_UNCOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

const UNCHANGED: &str = "Nenhum dado foi alterado.";

/// What the migration imported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LegacyImportReport {
    pub imported_at: String,
    pub questions: u32,
    pub collections: u32,
    pub collection_items: u32,
    pub topics: u32,
    pub images: u32,
}

/// Whether the migration ran, and whether the bank is still empty (first run).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LegacyImportStatus {
    pub report: Option<LegacyImportReport>,
    pub bank_empty: bool,
}

/// Result of [`import_legacy_bank`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LegacyImportOutcome {
    pub report: LegacyImportReport,
    /// `true` when the migration had already run; nothing was changed.
    pub already_imported: bool,
}

fn invalid(message: impl std::fmt::Display) -> AppError {
    AppError::Invalid(format!("{message} {UNCHANGED}"))
}

// ---- Legacy formats -------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    state: ManifestState,
    resources: Vec<ManifestResource>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestState {
    sha256: String,
    bytes: u64,
    uncompressed_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct ManifestResource {
    name: String,
    sha256: String,
    bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    format: String,
    version: u32,
    database_version: u32,
    tables: Tables,
}

#[derive(Debug, Deserialize)]
struct Tables {
    questions: Vec<QuestionRow>,
    versions: Vec<VersionRow>,
    collections: Vec<CollectionRow>,
    collection_items: Vec<ItemRow>,
    subjects: Vec<SubjectRow>,
    question_subjects: Vec<QuestionSubjectRow>,
}

#[derive(Debug, Deserialize)]
struct QuestionRow {
    id: String,
    data: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyQuestion {
    #[serde(rename = "type")]
    kind: String,
    stem: String,
    options: Vec<QuestionOption>,
    answer_id: String,
    #[serde(default)]
    explanation: String,
    #[serde(default)]
    refs: Vec<String>,
    image: Option<String>,
    image_alt: Option<String>,
    #[serde(default)]
    annulled: bool,
}

#[derive(Debug, Deserialize)]
struct VersionRow {
    id: String,
    question_id: String,
}

#[derive(Debug, Deserialize)]
struct CollectionRow {
    id: String,
    title: String,
    contest: String,
    created: String,
}

#[derive(Debug, Deserialize)]
struct ItemRow {
    collection_id: String,
    position: i64,
    question_id: String,
    version_id: String,
}

#[derive(Debug, Deserialize)]
struct SubjectRow {
    id: String,
    area: String,
    matter: String,
    topic: String,
}

#[derive(Debug, Deserialize)]
struct QuestionSubjectRow {
    question_id: String,
    subject_id: String,
}

// ---- Reading and verifying -------------------------------------------------------------------

/// Accepts the clone's root folder or its `biblioteca` folder.
fn locate_library(source: &Path) -> AppResult<PathBuf> {
    for candidate in [source.to_path_buf(), source.join("biblioteca")] {
        if candidate.join("manifesto.json").is_file() {
            return Ok(candidate);
        }
    }
    Err(invalid(
        "Não encontrei o banco do Caderno Local nesta pasta. Escolha a pasta do repositório \
         Simulados-Concursos (ou a pasta biblioteca dentro dele).",
    ))
}

fn read_limited(path: &Path, max: u64, label: &str) -> AppResult<Vec<u8>> {
    let size = fs::metadata(path)
        .map_err(|_| invalid(format!("Não foi possível ler {label}.")))?
        .len();
    if size > max {
        return Err(invalid(format!("{label} é grande demais ({size} bytes).")));
    }
    fs::read(path).map_err(|_| invalid(format!("Não foi possível ler {label}.")))
}

fn read_manifest(library: &Path) -> AppResult<Manifest> {
    let bytes = read_limited(
        &library.join("manifesto.json"),
        MAX_MANIFEST_BYTES,
        "manifesto.json",
    )?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| {
        invalid(format!(
            "O manifesto.json não está no formato esperado ({e})."
        ))
    })?;
    if manifest.format != "caderno-local" || manifest.version != 1 {
        return Err(invalid(format!(
            "Formato de manifesto não reconhecido ({} versão {}).",
            manifest.format, manifest.version
        )));
    }
    Ok(manifest)
}

fn read_snapshot(library: &Path, manifest: &Manifest) -> AppResult<Snapshot> {
    let state = &manifest.state;
    let compressed = read_limited(
        &library.join("estado.json.gz"),
        MAX_STATE_BYTES,
        "estado.json.gz",
    )?;
    if compressed.len() as u64 != state.bytes || sha256_hex(&compressed) != state.sha256 {
        return Err(invalid(
            "O arquivo estado.json.gz não confere com o manifesto (tamanho ou hash diferente).",
        ));
    }
    if state.uncompressed_bytes > MAX_UNCOMPRESSED_BYTES {
        return Err(invalid("O estado descompactado seria grande demais."));
    }
    let mut json = Vec::new();
    GzDecoder::new(compressed.as_slice())
        .take(state.uncompressed_bytes + 1)
        .read_to_end(&mut json)
        .map_err(|_| invalid("Não foi possível descompactar o estado.json.gz."))?;
    if json.len() as u64 != state.uncompressed_bytes {
        return Err(invalid(
            "O estado descompactado não tem o tamanho anunciado no manifesto.",
        ));
    }
    let snapshot: Snapshot = serde_json::from_slice(&json).map_err(|e| {
        invalid(format!(
            "O estado do Caderno Local não está no formato esperado ({e})."
        ))
    })?;
    if snapshot.format != "caderno-local" || snapshot.version != 1 || snapshot.database_version != 2
    {
        return Err(invalid(format!(
            "Versão do estado não suportada ({} {} / banco {}).",
            snapshot.format, snapshot.version, snapshot.database_version
        )));
    }
    Ok(snapshot)
}

struct Prepared {
    inputs: Vec<QuestionInput>,
    collections: Vec<CollectionRow>,
    items: Vec<ItemRow>,
    topics: usize,
    images: Vec<String>,
}

/// Maps the legacy tables to question inputs and checks the references between tables.
fn prepare(tables: Tables) -> AppResult<Prepared> {
    let subjects: HashMap<&str, &SubjectRow> =
        tables.subjects.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut topics_by_question: HashMap<&str, Vec<TopicPath>> = HashMap::new();
    for link in &tables.question_subjects {
        let subject = subjects.get(link.subject_id.as_str()).ok_or_else(|| {
            invalid(format!(
                "O assunto {} usado pela questão {} não existe.",
                link.subject_id, link.question_id
            ))
        })?;
        topics_by_question
            .entry(link.question_id.as_str())
            .or_default()
            .push(TopicPath::new(
                &subject.area,
                &subject.matter,
                &subject.topic,
            ));
    }

    let mut inputs = Vec::with_capacity(tables.questions.len());
    let mut images = Vec::new();
    for row in &tables.questions {
        let legacy: LegacyQuestion = serde_json::from_str(&row.data).map_err(|e| {
            invalid(format!(
                "A questão {} está em um formato inesperado ({e}).",
                row.id
            ))
        })?;
        let kind = QuestionType::parse(&legacy.kind).ok_or_else(|| {
            invalid(format!(
                "A questão {} é do tipo \"{}\", que o Gabaritei não importa.",
                row.id, legacy.kind
            ))
        })?;
        if let Some(image) = &legacy.image {
            images.push(image.clone());
        }
        inputs.push(QuestionInput {
            local_id: row.id.clone(),
            kind,
            stem: legacy.stem,
            options: legacy.options,
            answer_id: legacy.answer_id,
            explanation: legacy.explanation,
            refs: legacy.refs,
            topics: topics_by_question
                .remove(row.id.as_str())
                .unwrap_or_default(),
            image: legacy.image,
            image_alt: legacy.image_alt,
            annulled: legacy.annulled,
            provenance: Provenance::Legacy,
        });
    }

    let question_ids: HashSet<&str> = tables.questions.iter().map(|q| q.id.as_str()).collect();
    let collection_ids: HashSet<&str> = tables.collections.iter().map(|c| c.id.as_str()).collect();
    let versions: HashMap<&str, &str> = tables
        .versions
        .iter()
        .map(|v| (v.id.as_str(), v.question_id.as_str()))
        .collect();
    for item in &tables.collection_items {
        if !collection_ids.contains(item.collection_id.as_str())
            || !question_ids.contains(item.question_id.as_str())
            || versions.get(item.version_id.as_str()) != Some(&item.question_id.as_str())
        {
            return Err(invalid(format!(
                "O caderno {} referencia uma questão ou versão inexistente.",
                item.collection_id
            )));
        }
    }

    images.sort();
    images.dedup();
    Ok(Prepared {
        inputs,
        topics: tables.subjects.len(),
        collections: tables.collections,
        items: tables.collection_items,
        images,
    })
}

/// Reads and verifies every image the questions use.
fn read_images(
    library: &Path,
    manifest: &Manifest,
    names: &[String],
) -> AppResult<Vec<(String, Vec<u8>)>> {
    let resources: HashMap<&str, &ManifestResource> = manifest
        .resources
        .iter()
        .map(|r| (r.name.as_str(), r))
        .collect();
    names
        .iter()
        .map(|name| {
            let resource = resources
                .get(name.as_str())
                .ok_or_else(|| invalid(format!("A imagem {name} não está no manifesto.")))?;
            let stem = name.split('.').next().unwrap_or_default();
            if !is_valid_image_name(name) || !resource.sha256.starts_with(stem) {
                return Err(invalid(format!(
                    "Nome de imagem inválido no manifesto: {name}."
                )));
            }
            let bytes = read_limited(
                &library.join("recursos").join(name),
                MAX_IMAGE_BYTES,
                &format!("a imagem {name}"),
            )?;
            if bytes.len() as u64 != resource.bytes || sha256_hex(&bytes) != resource.sha256 {
                return Err(invalid(format!(
                    "A imagem {name} não confere com o manifesto (tamanho ou hash diferente)."
                )));
            }
            Ok((name.clone(), bytes))
        })
        .collect()
}

/// Writes images by content name; an existing file with the same content is kept.
fn write_images(images_dir: &Path, images: &[(String, Vec<u8>)]) -> AppResult<()> {
    fs::create_dir_all(images_dir)?;
    for (name, bytes) in images {
        let target = images_dir.join(name);
        if fs::read(&target).is_ok_and(|existing| &existing == bytes) {
            continue;
        }
        let temp = images_dir.join(format!("{name}.tmp"));
        fs::write(&temp, bytes)?;
        fs::rename(&temp, &target)?;
    }
    Ok(())
}

// ---- Entry points ----------------------------------------------------------------------------

/// Current migration status, for the first-run screen and settings.
pub fn status(db: &Database) -> AppResult<LegacyImportStatus> {
    db.with(|conn| {
        let report = get_setting::<LegacyImportReport>(conn, STATUS_KEY)?;
        let questions: i64 = conn.query_row("SELECT COUNT(*) FROM questions", [], |r| r.get(0))?;
        Ok(LegacyImportStatus {
            report,
            bank_empty: questions == 0,
        })
    })
}

/// Imports the legacy bank from `source` (the clone or its `biblioteca` folder) once.
/// A second call returns the first report and changes nothing.
pub fn import_legacy_bank(
    db: &Database,
    images_dir: &Path,
    source: &Path,
    now: &str,
) -> AppResult<LegacyImportOutcome> {
    if let Some(report) = db.with(|conn| get_setting(conn, STATUS_KEY))? {
        return Ok(LegacyImportOutcome {
            report,
            already_imported: true,
        });
    }

    let library = locate_library(source)?;
    let manifest = read_manifest(&library)?;
    let snapshot = read_snapshot(&library, &manifest)?;
    let prepared = prepare(snapshot.tables)?;
    let images = read_images(&library, &manifest, &prepared.images)?;

    write_import(db, images_dir, prepared, &images, now)
}

/// Writes the prepared bank in one transaction. The "already imported" check happens inside it,
/// so two imports that both passed the early check cannot both write: the database lock
/// serializes the transactions and the second one returns the first one's report. This keeps
/// the function safe on its own, whatever the caller (the command also runs it under the backup
/// lock, which already keeps two imports apart).
fn write_import(
    db: &Database,
    images_dir: &Path,
    prepared: Prepared,
    images: &[(String, Vec<u8>)],
    now: &str,
) -> AppResult<LegacyImportOutcome> {
    db.transaction(|tx| {
        if let Some(report) = get_setting(tx, STATUS_KEY)? {
            return Ok(LegacyImportOutcome {
                report,
                already_imported: true,
            });
        }
        let plan = plan_import(tx, prepared.inputs)?;
        if !plan.problems.is_empty() {
            let shown: Vec<&str> = plan.problems.iter().take(10).map(String::as_str).collect();
            return Err(invalid(format!(
                "O banco do Caderno Local tem {} problema(s), por exemplo:\n{}\n",
                plan.problems.len(),
                shown.join("\n")
            )));
        }
        let changed_ids = plan
            .questions
            .iter()
            .filter(|p| p.local_ids.iter().any(|local| local != &p.question.id))
            .count();
        if changed_ids > 0 {
            return Err(invalid(format!(
                "{changed_ids} questão(ões) não manteriam o identificador original do Caderno Local."
            )));
        }

        let outcome = apply_import(tx, &plan, now)?;

        for collection in &prepared.collections {
            tx.execute(
                "INSERT INTO collections (id, title, kind, contest, board, created_at)
                 VALUES (?1, ?2, 'imported', ?3, NULL, ?4)",
                params![collection.id, collection.title, collection.contest, collection.created],
            )?;
        }
        for item in &prepared.items {
            tx.execute(
                "INSERT INTO collection_items (collection_id, position, question_id)
                 VALUES (?1, ?2, ?3)",
                params![
                    item.collection_id,
                    item.position,
                    outcome.ids_by_local_id[&item.question_id]
                ],
            )?;
        }

        let report = LegacyImportReport {
            imported_at: now.to_owned(),
            questions: plan.questions.len() as u32,
            collections: prepared.collections.len() as u32,
            collection_items: prepared.items.len() as u32,
            topics: prepared.topics as u32,
            images: images.len() as u32,
        };
        // The last step before the commit that touches the database is done; writing the
        // images is the last fallible one, so a failure above (bad data, a conflicting row, the
        // status setting) rolls back without leaving image files behind. Only a failing commit
        // can still orphan them: harmless, because they are content-addressed and a retry
        // reuses them.
        set_setting(tx, STATUS_KEY, &report)?;
        write_images(images_dir, images)?;
        Ok(LegacyImportOutcome {
            report,
            already_imported: false,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank::identity::{fingerprint, option_id};
    use crate::bank::load_question;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use serde_json::{Value, json};
    use std::io::Write;

    const NOW: &str = "2026-11-19T12:00:00.000Z";
    /// id, kind, contest, board, created_at
    type CollectionColumns = (String, String, Option<String>, Option<String>, String);
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n fake image bytes";

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        images_dir: PathBuf,
        db: Database,
    }

    impl Fixture {
        fn library(&self) -> PathBuf {
            self.root.join("biblioteca")
        }
        fn count(&self, table: &str) -> i64 {
            self.db
                .with(|c| {
                    Ok(c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?)
                })
                .unwrap()
        }
    }

    fn image_name() -> String {
        format!("{}.png", &sha256_hex(PNG)[..32])
    }

    /// A canonical legacy question, as the old app stored it.
    fn legacy_question(
        stem: &str,
        texts: [&str; 5],
        answer: usize,
        image: bool,
    ) -> (String, Value) {
        let image = image.then(image_name);
        let id = fingerprint(QuestionType::Multiple, stem, texts, image.as_deref());
        let options: Vec<Value> = texts
            .iter()
            .map(|t| json!({ "id": option_id(t), "text": t }))
            .collect();
        let mut data = json!({
            "id": id, "type": "multiple", "stem": stem, "options": options,
            "answerId": option_id(texts[answer]), "explanation": "Comentário.",
            "refs": ["Lei 8.112/1990"], "subjects": [["Direito", "Administrativo", "Servidores"]],
            "tags": [], "objectives": ["objetivo"], "difficulty": "média", "annulled": false
        });
        if let Some(name) = image {
            data["image"] = json!(name);
            data["imageAlt"] = json!("Diagrama de exemplo.");
        }
        (id, data)
    }

    fn snapshot() -> Value {
        let (q1, d1) = legacy_question("Primeira?", ["A", "B", "C", "D", "E"], 1, true);
        let (q2, d2) = legacy_question("Segunda?", ["F", "G", "H", "I", "J"], 4, false);
        let row = |id: &str, data: &Value| json!({ "id": id, "fingerprint": id, "data": data.to_string() });
        json!({
            "format": "caderno-local", "version": 1, "databaseVersion": 2,
            "tables": {
                "questions": [row(&q1, &d1), row(&q2, &d2)],
                "versions": [
                    { "id": "v1", "question_id": q1, "data": "{}" },
                    { "id": "v2", "question_id": q2, "data": "{}" }
                ],
                "collections": [
                    { "id": "c-alepa", "title": "Direito › Servidores", "contest": "ALEPA · Programação", "board": "Acervo autoral", "kind": "bateria", "created": "2026-11-11T00:15:54.786Z" },
                    { "id": "c-perito", "title": "Servidores", "contest": "Perito Criminal", "board": "Acervo autoral", "kind": "bateria", "created": "2026-11-11T00:24:27.364Z" }
                ],
                "collection_items": [
                    { "collection_id": "c-alepa", "position": 0, "question_id": q1, "version_id": "v1" },
                    { "collection_id": "c-alepa", "position": 1, "question_id": q2, "version_id": "v2" },
                    { "collection_id": "c-perito", "position": 0, "question_id": q2, "version_id": "v2" }
                ],
                "subjects": [{ "id": "s1", "area": "Direito", "matter": "Administrativo", "topic": "Servidores" }],
                "question_subjects": [
                    { "question_id": q1, "subject_id": "s1" },
                    { "question_id": q2, "subject_id": "s1" }
                ],
                "attempts": [{ "id": "ignored" }], "responses": [], "drafts": [], "settings": []
            },
            "attemptOrder": []
        })
    }

    fn write_library(root: &Path, snapshot: &Value) {
        let library = root.join("biblioteca");
        fs::create_dir_all(library.join("recursos")).unwrap();
        let json = snapshot.to_string();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(json.as_bytes()).unwrap();
        let gz = encoder.finish().unwrap();
        fs::write(library.join("estado.json.gz"), &gz).unwrap();
        fs::write(library.join("recursos").join(image_name()), PNG).unwrap();
        let manifest = json!({
            "format": "caderno-local", "version": 1,
            "state": { "sha256": sha256_hex(&gz), "bytes": gz.len(), "uncompressedBytes": json.len() },
            "resources": [{ "name": image_name(), "sha256": sha256_hex(PNG), "bytes": PNG.len() }],
            "counts": {}
        });
        fs::write(library.join("manifesto.json"), manifest.to_string()).unwrap();
    }

    fn fixture_with(snapshot: &Value) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Simulados-Concursos");
        write_library(&root, snapshot);
        Fixture {
            images_dir: dir.path().join("app").join("images"),
            root,
            db: Database::open_in_memory().unwrap(),
            _dir: dir,
        }
    }

    fn run(f: &Fixture) -> AppResult<LegacyImportOutcome> {
        import_legacy_bank(&f.db, &f.images_dir, &f.root, NOW)
    }

    fn assert_nothing_changed(f: &Fixture) {
        for table in [
            "questions",
            "topics",
            "collections",
            "collection_items",
            "settings",
        ] {
            assert_eq!(f.count(table), 0, "{table}");
        }
        assert!(!f.images_dir.exists(), "no image was copied");
    }

    #[test]
    fn a_failure_after_the_questions_are_applied_leaves_no_image_behind() {
        let f = fixture_with(&snapshot());
        // A collection that already exists makes the collection insert fail, after the
        // questions were applied and before the commit.
        f.db.with(|c| {
            c.execute(
                "INSERT INTO collections (id, title, kind, created_at)
                 VALUES ('c-alepa', 'Já existe', 'practice', ?1)",
                [NOW],
            )?;
            Ok(())
        })
        .unwrap();
        assert!(run(&f).is_err());
        assert_eq!(f.count("questions"), 0, "the transaction rolled back");
        assert!(!f.images_dir.exists(), "no image was copied");
    }

    #[test]
    fn maps_questions_topics_collections_and_images() {
        let f = fixture_with(&snapshot());
        let outcome = run(&f).unwrap();
        assert!(!outcome.already_imported);
        assert_eq!(
            outcome.report,
            LegacyImportReport {
                imported_at: NOW.into(),
                questions: 2,
                collections: 2,
                collection_items: 3,
                topics: 1,
                images: 1
            }
        );

        let (q1, _) = legacy_question("Primeira?", ["A", "B", "C", "D", "E"], 1, true);
        let stored = f.db.with(|c| load_question(c, &q1)).unwrap().unwrap();
        assert_eq!(stored.id, q1, "legacy ID kept");
        assert_eq!(stored.answer_text(), Some("B"));
        assert_eq!(stored.explanation, "Comentário.");
        assert_eq!(stored.refs, ["Lei 8.112/1990"]);
        assert_eq!(
            stored.topics,
            vec![TopicPath::new("Direito", "Administrativo", "Servidores")]
        );
        assert_eq!(stored.image_alt.as_deref(), Some("Diagrama de exemplo."));
        assert_eq!(stored.provenance, Provenance::Legacy);
        assert_eq!(fs::read(f.images_dir.join(image_name())).unwrap(), PNG);

        f.db.with(|c| {
            let rows: Vec<CollectionColumns> = c
                .prepare(
                    "SELECT id, kind, contest, board, created_at FROM collections ORDER BY id",
                )?
                .query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })?
                .collect::<Result<_, _>>()?;
            assert_eq!(
                rows[0],
                (
                    "c-alepa".into(),
                    "imported".into(),
                    Some("ALEPA · Programação".into()),
                    None,
                    "2026-11-11T00:15:54.786Z".into()
                )
            );
            assert_eq!(rows[1].2.as_deref(), Some("Perito Criminal"));
            let shared: i64 = c.query_row(
                "SELECT COUNT(DISTINCT collection_id) FROM collection_items ci
                 JOIN questions q ON q.id = ci.question_id WHERE q.stem = 'Segunda?'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(
                shared, 2,
                "a shared question belongs to both contests' collections"
            );
            Ok(())
        })
        .unwrap();
        assert_eq!(f.count("questions"), 2);
    }

    #[test]
    fn running_twice_changes_nothing() {
        let f = fixture_with(&snapshot());
        let first = run(&f).unwrap();
        fs::remove_dir_all(f.library()).unwrap();

        let second = run(&f).unwrap();
        assert!(second.already_imported);
        assert_eq!(second.report, first.report);
        assert_eq!(f.count("questions"), 2);
        assert_eq!(f.count("collection_items"), 3);
    }

    #[test]
    fn an_import_that_lost_the_race_writes_nothing_and_returns_the_winner_report() {
        // Another import finishes between this one's early status check and its write.
        let f = fixture_with(&snapshot());
        let winner = run(&f).unwrap().report;
        let library = locate_library(&f.root).unwrap();
        let manifest = read_manifest(&library).unwrap();
        let snapshot = read_snapshot(&library, &manifest).unwrap();
        let prepared = prepare(snapshot.tables).unwrap();
        let images = read_images(&library, &manifest, &prepared.images).unwrap();
        fs::remove_dir_all(&f.images_dir).unwrap();

        let outcome = write_import(
            &f.db,
            &f.images_dir,
            prepared,
            &images,
            "2030-01-01T00:00:00.000Z",
        )
        .unwrap();

        assert!(outcome.already_imported);
        assert_eq!(outcome.report, winner);
        assert_eq!(f.count("questions"), 2);
        assert_eq!(f.count("collections"), 2);
        assert_eq!(f.count("collection_items"), 3);
        assert!(!f.images_dir.exists(), "the loser copied no image");
    }

    #[test]
    fn accepts_the_biblioteca_folder_itself() {
        let f = fixture_with(&snapshot());
        let outcome = import_legacy_bank(&f.db, &f.images_dir, &f.library(), NOW).unwrap();
        assert_eq!(outcome.report.questions, 2);
    }

    #[test]
    fn a_tampered_state_file_changes_nothing() {
        let f = fixture_with(&snapshot());
        let path = f.library().join("estado.json.gz");
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        fs::write(&path, bytes).unwrap();

        let error = run(&f).unwrap_err().to_string();
        assert!(
            error.contains("estado.json.gz não confere com o manifesto"),
            "{error}"
        );
        assert!(error.ends_with(UNCHANGED), "{error}");
        assert_nothing_changed(&f);
    }

    #[test]
    fn a_corrupted_image_changes_nothing() {
        let f = fixture_with(&snapshot());
        fs::write(
            f.library().join("recursos").join(image_name()),
            b"other bytes",
        )
        .unwrap();
        let error = run(&f).unwrap_err().to_string();
        assert!(error.contains("não confere com o manifesto"), "{error}");
        assert_nothing_changed(&f);
    }

    #[test]
    fn a_question_that_breaks_the_rules_changes_nothing() {
        let mut snap = snapshot();
        let data = snap["tables"]["questions"][1]["data"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut question: Value = serde_json::from_str(&data).unwrap();
        question["answerId"] = json!("does-not-exist");
        snap["tables"]["questions"][1]["data"] = json!(question.to_string());
        let f = fixture_with(&snap);

        let error = run(&f).unwrap_err().to_string();
        assert!(error.contains("O gabarito não corresponde"), "{error}");
        assert_nothing_changed(&f);
    }

    #[test]
    fn a_question_whose_id_is_not_its_fingerprint_changes_nothing() {
        let mut snap = snapshot();
        let original = snap["tables"]["questions"][1]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let forged = "f".repeat(64);
        for path in [
            "/tables/questions/1/id",
            "/tables/versions/1/question_id",
            "/tables/collection_items/1/question_id",
            "/tables/collection_items/2/question_id",
            "/tables/question_subjects/1/question_id",
        ] {
            assert_eq!(snap.pointer(path).unwrap(), &json!(original), "{path}");
            *snap.pointer_mut(path).unwrap() = json!(forged);
        }
        let f = fixture_with(&snap);
        let error = run(&f).unwrap_err().to_string();
        assert!(
            error.contains("não manteriam o identificador original"),
            "{error}"
        );
        assert_nothing_changed(&f);
    }

    #[test]
    fn unsupported_formats_and_broken_references_change_nothing() {
        let mut wrong_version = snapshot();
        wrong_version["databaseVersion"] = json!(3);
        let mut essay = snapshot();
        let data = essay["tables"]["questions"][0]["data"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut question: Value = serde_json::from_str(&data).unwrap();
        question["type"] = json!("essay");
        essay["tables"]["questions"][0]["data"] = json!(question.to_string());
        let mut dangling = snapshot();
        dangling["tables"]["collection_items"][0]["version_id"] = json!("v-missing");

        for (snap, expected) in [
            (wrong_version, "Versão do estado não suportada"),
            (essay, "do tipo \"essay\""),
            (dangling, "referencia uma questão ou versão inexistente"),
        ] {
            let f = fixture_with(&snap);
            let error = run(&f).unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
            assert_nothing_changed(&f);
        }
    }

    #[test]
    fn a_folder_without_the_bank_is_explained() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        let error = import_legacy_bank(&db, &dir.path().join("images"), dir.path(), NOW)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("Não encontrei o banco do Caderno Local"),
            "{error}"
        );
    }

    #[test]
    fn status_reports_first_run_and_then_the_import() {
        let f = fixture_with(&snapshot());
        assert_eq!(
            status(&f.db).unwrap(),
            LegacyImportStatus {
                report: None,
                bank_empty: true
            }
        );
        let outcome = run(&f).unwrap();
        assert_eq!(
            status(&f.db).unwrap(),
            LegacyImportStatus {
                report: Some(outcome.report),
                bank_empty: false
            }
        );
    }

    /// Imports the real legacy bank. Run with
    /// `LEGACY_CLONE=/path/to/Simulados-Concursos cargo test real_legacy_bank -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a clone of the legacy repository"]
    fn real_legacy_bank() {
        let clone = std::env::var("LEGACY_CLONE").expect("set LEGACY_CLONE");
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("gabaritei.sqlite")).unwrap();
        let started = std::time::Instant::now();
        let outcome = import_legacy_bank(&db, &dir.path().join("images"), Path::new(&clone), NOW)
            .expect("real bank imports");
        let elapsed = started.elapsed();
        let counts = db
            .with(|c| {
                let n = |t: &str| -> rusqlite::Result<i64> {
                    c.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
                };
                Ok([
                    n("questions")?,
                    n("collections")?,
                    n("collection_items")?,
                    n("topics")?,
                    n("question_topics")?,
                ])
            })
            .unwrap();
        let images = fs::read_dir(dir.path().join("images")).unwrap().count();
        println!("report: {:?}", outcome.report);
        println!(
            "tables [questions, collections, items, topics, question_topics]: {counts:?}; image files: {images}; took {elapsed:?}"
        );
        assert_eq!(counts[..4], [9_368, 352, 9_757, 344]);
        assert_eq!(images, 52);
    }
}
