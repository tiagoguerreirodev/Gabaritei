use super::*;
use rusqlite::params;

thread_local! {
    /// Overrides [`computer_tag`] for the current test: `Some(None)` is "no tag".
    /// Without an override, tests make untagged backups.
    pub(super) static TAG: std::cell::RefCell<Option<Option<String>>> =
        const { std::cell::RefCell::new(None) };
}

thread_local! {
    /// Overrides [`previous_tag`] for the current test.
    pub(super) static PREVIOUS: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// The per-thread test overrides, to copy onto a probe's worker thread.
pub(super) fn overrides() -> (Option<Option<String>>, Option<String>) {
    (
        TAG.with(|tag| tag.borrow().clone()),
        PREVIOUS.with(|tag| tag.borrow().clone()),
    )
}

pub(super) fn apply((tag, previous): (Option<Option<String>>, Option<String>)) {
    TAG.with(|current| *current.borrow_mut() = tag);
    PREVIOUS.with(|current| *current.borrow_mut() = previous);
}

pub(super) fn as_computer(tag: &str) {
    TAG.with(|current| *current.borrow_mut() = Some(Some(tag.to_owned())));
}

pub(super) struct Fixture {
    pub(super) _dir: tempfile::TempDir,
    pub(super) paths: AppPaths,
    pub(super) db: Database,
}

pub(super) const IMAGE: &str = "0123456789abcdef01234567.png";

pub(super) fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(dir.path().join("data"));
    let db = Database::open(&paths.database()).unwrap();
    fs::create_dir_all(paths.images_dir()).unwrap();
    fs::write(paths.images_dir().join(IMAGE), b"png bytes").unwrap();
    Fixture {
        _dir: dir,
        paths,
        db,
    }
}

pub(super) fn add_question(db: &Database, id: &str) {
    db.with(|conn| {
        conn.execute(
            "INSERT INTO questions (id, type, stem, options, answer_id, created_at, updated_at)
             VALUES (?1, ?2, 'Enunciado', '[{\"id\":\"c\",\"text\":\"Certo\"},{\"id\":\"e\",\"text\":\"Errado\"}]',
                     'c', '2026-11-19T00:00:00Z', '2026-11-19T00:00:00Z')",
            params![id, crate::bank::QuestionType::Boolean],
        )?;
        Ok(())
    })
    .unwrap();
}

pub(super) fn question_ids(db: &Database) -> Vec<String> {
    db.with(|conn| {
        let mut stmt = conn.prepare("SELECT id FROM questions ORDER BY id")?;
        Ok(stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?)
    })
    .unwrap()
}

pub(super) fn backup(f: &Fixture, now: &str) -> BackupInfo {
    let folder = default_folder(&f.paths);
    create(&f.db, &f.paths, &folder, BackupKind::Backup, now, true).unwrap()
}

/// Rewrites the backup's database with another page size and fixes the manifest hash, like a
/// file made by a build that uses a different page size.
pub(super) fn change_page_size(folder: &Path, name: &str) {
    let dir = folder.join(name);
    let database = dir.join(snapshot::DATABASE_FILE);
    let conn = Connection::open(&database).unwrap();
    conn.pragma_update(None, "page_size", 8192).unwrap();
    conn.execute_batch("VACUUM").unwrap();
    conn.close().unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join(snapshot::MANIFEST_FILE)).unwrap()).unwrap();
    let (sha256, size) = snapshot::hash_file(&database).unwrap();
    for file in manifest["files"].as_array_mut().unwrap() {
        if file["path"] == snapshot::DATABASE_FILE {
            file["sha256"] = sha256.clone().into();
            file["size"] = size.into();
        }
    }
    fs::write(
        dir.join(snapshot::MANIFEST_FILE),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
}

/// Runs `sql` on a backup's database and fixes the manifest hash, as someone building a
/// hostile file would.
pub(super) fn doctor(f: &Fixture, info: &BackupInfo, sql: &str) {
    let dir = default_folder(&f.paths).join(&info.name);
    let database = dir.join(snapshot::DATABASE_FILE);
    Connection::open(&database)
        .unwrap()
        .execute_batch(sql)
        .unwrap();
    let mut manifest = snapshot::read_manifest(&dir).unwrap();
    let (sha256, size) = snapshot::hash_file(&database).unwrap();
    manifest.files[0].sha256 = sha256;
    manifest.files[0].size = size;
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
}

pub(super) fn folder_of(conn: &Connection, paths: &AppPaths) -> AppResult<PathBuf> {
    folder(conn, paths)
}

pub(super) fn make_left_behind(f: &Fixture, service: &BackupService) -> (PathBuf, PathBuf) {
    let old = default_folder(&f.paths);
    for (kind, at) in [
        (BackupKind::Backup, "2026-11-19T10:00:00.000Z"),
        (BackupKind::Safety, "2026-11-19T11:00:00.000Z"),
    ] {
        create(&f.db, &f.paths, &old, kind, at, false).unwrap();
    }
    let new = f._dir.path().join("synced");
    service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();
    (old, new)
}

/// Work that blocks until the returned sender is dropped or used: a stand-in for a read
/// stuck on a sleeping drive, released by the test and not by the clock.
pub(super) fn blocked() -> (mpsc::Sender<()>, impl FnOnce() -> u32 + Send + 'static) {
    let (release, wait) = mpsc::channel::<()>();
    (release, move || {
        let _ = wait.recv();
        7
    })
}

/// Waits (briefly, polling) until `done` is true.
pub(super) fn eventually(done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "the condition never became true");
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(super) fn occupied(probe: &FolderProbe) -> bool {
    !lock(&probe.probes.0).is_empty()
}

/// Long enough that no test depends on the clock: stuck folders are made with
/// [`FolderProbe::mark_stuck`] or work that blocks until released.
pub(super) const LONG: Duration = Duration::from_secs(60);

pub(super) fn service_with_long_probe() -> BackupService {
    BackupService::default()
}

impl BackupService {
    pub(super) fn with_cleanup(cleanup: RestoreCleanup) -> Self {
        Self::with_timeouts(LONG, LONG, cleanup)
    }

    pub(super) fn relocate_with(
        &self,
        db: &Database,
        paths: &AppPaths,
        mode: Relocation,
        between: &dyn Fn(),
    ) -> AppResult<RelocateOutcome> {
        let _running = Relocating::start(&self.relocating)?;
        let plan = self.exclusive(|| self.plan_relocation(db, paths))?;
        let report = plan.execute(mode, &|work| {
            self.relocation_turn(db, paths, &plan, work);
            between();
        })?;
        self.finish_relocation(db, paths, &plan, mode, report)
    }
}

impl Default for BackupService {
    fn default() -> Self {
        Self::with_cleanup(crate::cleanup_restored_data)
    }
}
