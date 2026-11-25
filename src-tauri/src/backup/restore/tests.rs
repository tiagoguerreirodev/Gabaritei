use super::*;
use crate::backup::fixtures::*;

fn restore_fixture(f: &Fixture, name: &str, now: &str) -> AppResult<()> {
    BackupService::default().restore(&f.db, &f.paths, name, instant(now), || Ok(()))
}

#[test]
fn a_cleanup_failure_keeps_the_live_database() {
    let f = fixture();
    add_question(&f.db, "q-1");
    let info = backup(&f, "2026-11-19T10:00:00.000Z");
    add_question(&f.db, "q-2");
    let service = BackupService::with_cleanup(|conn| {
        conn.execute("DELETE FROM questions", [])?;
        Err(AppError::Invalid(
            "Revisões não puderam ser reconstruídas.".into(),
        ))
    });

    let error = service
        .restore(
            &f.db,
            &f.paths,
            &info.name,
            instant("2026-11-19T11:00:00.000Z"),
            || Ok(()),
        )
        .unwrap_err();

    assert_eq!(error.to_string(), "Revisões não puderam ser reconstruídas.");
    assert_eq!(question_ids(&f.db), ["q-1", "q-2"]);
}

#[test]
fn a_consistent_backup_restores_the_answers_and_passes_the_integrity_check() {
    let f = fixture();
    add_question(&f.db, "q-1");
    add_question(&f.db, "q-2");
    let info = backup(&f, "2026-11-19T10:00:00.000Z");
    assert_eq!(info.name, "backup-20261119-100000-000");

    // The data changes after the backup: one question is lost, another one added.
    f.db.with(|conn| {
        conn.execute("DELETE FROM questions WHERE id = 'q-2'", [])?;
        Ok(())
    })
    .unwrap();
    add_question(&f.db, "q-3");

    restore_fixture(&f, &info.name, "2026-11-19T11:00:00.000Z").unwrap();

    assert_eq!(question_ids(&f.db), ["q-1", "q-2"]);
    f.db.with(|conn| {
        let integrity: String =
            conn.pragma_query_value(None, "integrity_check", |row| row.get(0))?;
        assert_eq!(integrity, "ok");
        let mode: String = conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        assert_eq!(mode, "wal");
        Ok(())
    })
    .unwrap();
    // The database keeps working after the replacement.
    add_question(&f.db, "q-4");
}

#[test]
fn a_page_size_mismatch_is_refused_before_the_safety_backup_so_nothing_changes() {
    let f = fixture();
    add_question(&f.db, "q-1");
    let info = backup(&f, "2026-11-19T10:00:00.000Z");
    let folder = default_folder(&f.paths);
    change_page_size(&folder, &info.name);
    add_question(&f.db, "q-2");

    let error = restore_fixture(&f, &info.name, "2026-11-19T11:00:00.000Z").unwrap_err();

    assert!(matches!(error, AppError::Invalid(_)), "{error:?}");
    assert!(error.to_string().contains("Nada foi alterado"));
    assert_eq!(question_ids(&f.db), ["q-1", "q-2"]);
    assert!(
        list(&folder)
            .iter()
            .all(|info| info.kind != BackupKind::Safety),
        "no safety backup for a replacement that was refused"
    );
}

#[test]
fn changing_the_folder_reports_the_backups_left_in_the_old_one() {
    let f = fixture();
    let service = BackupService::default();
    let old = default_folder(&f.paths);
    create(
        &f.db,
        &f.paths,
        &old,
        BackupKind::Backup,
        &Computer::default(),
        instant("2026-11-19T10:00:00.000Z"),
    )
    .unwrap();
    create(
        &f.db,
        &f.paths,
        &old,
        BackupKind::Safety,
        &Computer::default(),
        instant("2026-11-19T11:00:00.000Z"),
    )
    .unwrap();
    let elsewhere = f._dir.path().join("synced");

    let status = service
        .change_folder(&f.db, &f.paths, Some(&elsewhere))
        .unwrap();

    assert_eq!(status.folder, elsewhere.display().to_string());
    assert_eq!(
        status.listing,
        BackupListing::Ready {
            backups: Vec::new()
        }
    );
    assert_eq!(
        status.left_behind,
        Some(LeftBehind::Available {
            folder: old.display().to_string(),
            backups: 2,
            safety: 1,
        })
    );
    // Nothing was moved or deleted.
    assert_eq!(list(&old).len(), 2);
    // The plain status carries it too (it is remembered, see #111).
    assert_eq!(
        service.status(&f.db, &f.paths).unwrap().left_behind,
        status.left_behind
    );
    // Choosing the same folder again, or an empty old folder, has nothing left behind.
    let again = service
        .change_folder(&f.db, &f.paths, Some(&elsewhere))
        .unwrap();
    assert_eq!(again.left_behind, None);
    let back = service.change_folder(&f.db, &f.paths, None).unwrap();
    assert_eq!(back.left_behind, None);
    assert!(back.is_default_folder);
}

#[test]
fn restoring_makes_a_safety_backup_of_what_it_replaces() {
    let f = fixture();
    add_question(&f.db, "q-1");
    let info = backup(&f, "2026-11-19T10:00:00.000Z");
    add_question(&f.db, "q-2");

    restore_fixture(&f, &info.name, "2026-11-19T11:00:00.000Z").unwrap();

    let all = list(&default_folder(&f.paths));
    let safety = all
        .iter()
        .find(|info| info.kind == BackupKind::Safety)
        .expect("a safety backup exists");
    assert_eq!(safety.name, "safety-20261119-110000-000");
    // Restoring the safety backup brings the replaced data back.
    restore_fixture(&f, &safety.name, "2026-11-19T12:00:00.000Z").unwrap();
    assert_eq!(question_ids(&f.db), ["q-1", "q-2"]);
}

#[test]
fn a_corrupt_backup_changes_nothing() {
    let f = fixture();
    add_question(&f.db, "q-1");
    let info = backup(&f, "2026-11-19T10:00:00.000Z");
    add_question(&f.db, "q-2");
    let folder = default_folder(&f.paths);
    let dir = folder.join(&info.name);

    // A flipped byte in the database fails the hash.
    let database = dir.join(snapshot::DATABASE_FILE);
    let mut bytes = fs::read(&database).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    fs::write(&database, bytes).unwrap();

    let error = restore_fixture(&f, &info.name, "2026-11-19T11:00:00.000Z").unwrap_err();
    assert!(error.to_string().contains("corrompidos"), "{error}");
    assert_eq!(question_ids(&f.db), ["q-1", "q-2"]);
    assert!(
        list(&folder).iter().all(|b| b.kind == BackupKind::Backup),
        "no safety backup for a backup that was rejected"
    );
}

#[test]
fn a_backup_with_a_bad_database_or_missing_files_is_rejected() {
    let f = fixture();
    add_question(&f.db, "q-1");
    let folder = default_folder(&f.paths);

    // Missing image.
    let info = backup(&f, "2026-11-19T10:00:00.000Z");
    fs::remove_file(folder.join(&info.name).join("images").join(IMAGE)).unwrap();
    assert!(snapshot::verify(&folder.join(&info.name)).is_err());

    // A database that is not SQLite, with a matching hash.
    let info = backup(&f, "2026-11-19T10:01:00.000Z");
    let dir = folder.join(&info.name);
    let database = dir.join(snapshot::DATABASE_FILE);
    fs::write(&database, b"definitely not a database").unwrap();
    let mut manifest = snapshot::read_manifest(&dir).unwrap();
    let (sha256, size) = snapshot::hash_file(&database).unwrap();
    manifest.files[0].sha256 = sha256;
    manifest.files[0].size = size;
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(snapshot::verify(&dir).is_err());

    // A manifest that points outside the snapshot.
    let info = backup(&f, "2026-11-19T10:02:00.000Z");
    let dir = folder.join(&info.name);
    let mut manifest = snapshot::read_manifest(&dir).unwrap();
    manifest.files[0].path = "../gabaritei.sqlite".into();
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(snapshot::verify(&dir).is_err());

    assert_eq!(question_ids(&f.db), ["q-1"]);
}

#[test]
fn a_database_with_another_schema_is_rejected_before_anything_changes() {
    for sql in [
        "DROP TABLE settings",
        "CREATE TRIGGER evil AFTER INSERT ON responses BEGIN DELETE FROM settings; END",
        "CREATE VIEW sneaky AS SELECT 1",
        // A name that a `LIKE 'sqlite_%'` filter would have taken for an internal one.
        "CREATE TRIGGER sqliteXevil AFTER INSERT ON responses BEGIN DELETE FROM settings; END",
        "CREATE TABLE sqliteXextra (x TEXT) STRICT",
        "CREATE TABLE extra (x TEXT) STRICT",
        // A row whose `type` says index but whose SQL is a trigger, as a hand-built file could.
        "PRAGMA writable_schema = ON; INSERT INTO sqlite_schema VALUES ('index', 'sqlite_evil', 'responses', 0, 'CREATE TRIGGER sqlite_evil AFTER INSERT ON responses BEGIN DELETE FROM settings; END')",
    ] {
        let f = fixture();
        add_question(&f.db, "q-1");
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        doctor(&f, &info, sql);
        add_question(&f.db, "q-2");
        let folder = default_folder(&f.paths);

        let error = restore_fixture(&f, &info.name, "2026-11-19T11:00:00.000Z").unwrap_err();

        assert!(
            error.to_string().contains("Nada foi alterado"),
            "{sql}: {error}"
        );
        assert_eq!(question_ids(&f.db), ["q-1", "q-2"], "{sql}");
        assert!(
            list(&folder).iter().all(|b| b.kind == BackupKind::Backup),
            "{sql}: no safety backup"
        );
        // The app still works: its settings and its backup folder are intact.
        f.db.with(|conn| folder_of(conn, &f.paths)).unwrap();
    }
}

#[test]
fn a_backup_from_a_newer_schema_is_rejected() {
    let f = fixture();
    let info = backup(&f, "2026-11-19T10:00:00.000Z");
    let dir = default_folder(&f.paths).join(&info.name);
    let mut manifest = snapshot::read_manifest(&dir).unwrap();
    manifest.schema_version = crate::db::SCHEMA_VERSION + 1;
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let error = snapshot::verify(&dir).unwrap_err();
    assert_eq!(error.to_string(), crate::db::NEWER_SCHEMA_MESSAGE);
}

#[test]
fn restore_only_accepts_backup_names() {
    let f = fixture();
    for name in [
        "../x",
        "backup-1",
        "other",
        "backup-20261119-100000-00x",
        "",
    ] {
        assert!(restore_fixture(&f, name, "2026-11-19T11:00:00.000Z").is_err());
    }
}

#[test]
fn images_copied_ahead_of_the_lock_are_kept_and_later_ones_are_added() {
    let f = fixture();
    let late = "fedcba9876543210fedcba98.png";
    let dest = f.paths.data_dir().join("snapshot");
    fs::create_dir_all(&dest).unwrap();
    // Early copy: only IMAGE exists yet.
    let ahead = snapshot::copy_images_ahead(&f.paths.images_dir(), &dest).unwrap();
    assert_eq!(ahead.len(), 1);
    // An image added between the early copy and the database copy.
    fs::write(f.paths.images_dir().join(late), b"late bytes").unwrap();
    let manifest =
        f.db.with(|conn| {
            snapshot::write_locked(
                conn,
                &f.paths.images_dir(),
                &dest,
                "2026-11-19T10:00:00.000Z",
                ahead,
            )
        })
        .unwrap();
    let paths: Vec<&str> = manifest
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "gabaritei.sqlite",
            &format!("images/{IMAGE}"),
            &format!("images/{late}")
        ]
    );
    // The snapshot is complete and every hash matches.
    snapshot::verify(&dest).unwrap();
}

#[test]
fn a_restore_still_puts_every_image_in_its_safety_backup() {
    let f = fixture();
    let info = backup(&f, "2026-11-19T10:00:00.000Z");
    let extra = "fedcba9876543210fedcba98.png";
    fs::write(f.paths.images_dir().join(extra), b"extra").unwrap();
    restore_fixture(&f, &info.name, "2026-11-19T11:00:00.000Z").unwrap();
    let safety = default_folder(&f.paths).join("safety-20261119-110000-000");
    assert_eq!(
        fs::read(safety.join("images").join(extra)).unwrap(),
        b"extra"
    );
    assert_eq!(
        fs::read(safety.join("images").join(IMAGE)).unwrap(),
        b"png bytes"
    );
    snapshot::verify(&safety).unwrap();
    assert!(
        !default_folder(&f.paths).read_dir().unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".tmp-")),
        "no temporary folder left"
    );
}

#[test]
fn images_are_backed_up_and_come_back() {
    let f = fixture();
    let info = backup(&f, "2026-11-19T10:00:00.000Z");
    let dir = default_folder(&f.paths).join(&info.name);
    assert_eq!(
        fs::read(dir.join("images").join(IMAGE)).unwrap(),
        b"png bytes"
    );
    // Not an image name: never copied.
    fs::write(f.paths.images_dir().join("notes.txt"), b"x").unwrap();
    let second = backup(&f, "2026-11-19T10:01:00.000Z");
    assert!(
        !default_folder(&f.paths)
            .join(&second.name)
            .join("images/notes.txt")
            .exists()
    );

    fs::remove_file(f.paths.images_dir().join(IMAGE)).unwrap();
    restore_fixture(&f, &info.name, "2026-11-19T11:00:00.000Z").unwrap();
    assert_eq!(
        fs::read(f.paths.images_dir().join(IMAGE)).unwrap(),
        b"png bytes"
    );
}

#[test]
fn restoring_keeps_this_computers_own_settings() {
    let f = fixture();
    f.db.with(|conn| {
        set_setting(conn, SettingKey::CliPaths, &"windows-path")?;
        set_setting(conn, SettingKey::RemNote, &"http://remote.example/mcp")?;
        set_setting(conn, SettingKey::AiRoles, &"roles from the other computer")?;
        set_setting(
            conn,
            SettingKey::Appearance,
            &"dark and xl from the other computer",
        )?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('other', ?1)",
            [r#""from the backup""#],
        )?;
        Ok(())
    })
    .unwrap();
    let info = backup(&f, "2026-11-19T10:00:00.000Z");
    f.db.with(|conn| {
        set_setting(conn, SettingKey::CliPaths, &"mac-path")?;
        set_setting(conn, SettingKey::RemNote, &"http://localhost/mcp")?;
        set_setting(conn, SettingKey::AiRoles, &"roles of this computer")?;
        set_setting(
            conn,
            SettingKey::Appearance,
            &"light and m of this computer",
        )?;
        conn.execute(
            "UPDATE settings SET value = ?1 WHERE key = 'other'",
            [r#""changed""#],
        )?;
        set_setting(conn, SettingKey::BackupFolder, &"/elsewhere")
    })
    .unwrap();

    // The folder setting points at /elsewhere now, so the backup is found by moving it.
    let elsewhere = tempfile::tempdir().unwrap();
    f.db.with(|conn| {
        set_setting(
            conn,
            SettingKey::BackupFolder,
            &elsewhere.path().to_str().unwrap(),
        )
    })
    .unwrap();
    fs::rename(
        default_folder(&f.paths).join(&info.name),
        elsewhere.path().join(&info.name),
    )
    .unwrap();
    restore_fixture(&f, &info.name, "2026-11-19T11:00:00.000Z").unwrap();

    f.db.with(|conn| {
        assert_eq!(
            conn.query_row(
                "SELECT value FROM settings WHERE key = 'other'",
                [],
                |row| row.get::<_, String>(0)
            )?,
            r#""from the backup""#
        );
        assert_eq!(
            get_setting::<String>(conn, SettingKey::CliPaths)?.as_deref(),
            Some("mac-path")
        );
        assert_eq!(
            get_setting::<String>(conn, SettingKey::RemNote)?.as_deref(),
            Some("http://localhost/mcp")
        );
        assert_eq!(
            get_setting::<String>(conn, SettingKey::AiRoles)?.as_deref(),
            Some("roles of this computer")
        );
        assert_eq!(
            get_setting::<String>(conn, SettingKey::Appearance)?.as_deref(),
            Some("light and m of this computer")
        );
        assert_eq!(
            get_setting::<String>(conn, SettingKey::BackupFolder)?.as_deref(),
            elsewhere.path().to_str()
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn restoring_rebuilds_reviews_from_the_restored_answers() {
    let f = fixture();
    add_question(&f.db, "q-1");
    // A review the answers don't justify, as an older build could have left in a backup.
    f.db.with(|conn| {
        conn.execute(
            "INSERT INTO review_state (question_id, due_at, state)
             VALUES ('q-1', '2026-11-20T00:00:00Z', '{}')",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let info = backup(&f, "2026-11-19T10:00:00.000Z");
    restore_fixture(&f, &info.name, "2026-11-19T11:00:00.000Z").unwrap();
    let reviews: i64 = f
        .db
        .with(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM review_state", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(reviews, 0);
}
