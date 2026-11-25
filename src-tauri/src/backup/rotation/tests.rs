use super::*;
use crate::backup::fixtures::*;
use time::macros::datetime;

#[test]
fn keeps_the_newest_seven_backups_and_five_safety_backups() {
    let f = fixture();
    for day in 1..=9 {
        backup(&f, &format!("2026-11-{day:02}T10:00:00.000Z"));
    }
    let folder = default_folder(&f.paths);
    let names: Vec<String> = list(&folder).into_iter().map(|b| b.name).collect();
    assert_eq!(names.len(), 7);
    assert_eq!(names[0], "backup-20261109-100000-000");
    assert_eq!(names[6], "backup-20261103-100000-000");

    for minute in 0..7 {
        restore(
            &f.db,
            &f.paths,
            "backup-20261109-100000-000",
            &format!("2026-11-10T10:{minute:02}:00.000Z"),
        )
        .unwrap();
    }
    let all = list(&folder);
    assert_eq!(
        all.iter().filter(|b| b.kind == BackupKind::Safety).count(),
        5
    );
    assert_eq!(
        all.iter().filter(|b| b.kind == BackupKind::Backup).count(),
        7
    );
}

#[test]
fn manual_backups_are_kept_apart_from_the_daily_ones() {
    let f = fixture();
    let folder = default_folder(&f.paths);
    let service = BackupService::default();
    for day in 1..=3 {
        backup(&f, &format!("2026-11-{day:02}T10:00:00.000Z"));
    }
    // Pressing the button many times never pushes a daily backup out...
    for minute in 0..9 {
        let made = service
            .backup_now(
                &f.db,
                &f.paths,
                &format!("2026-11-04T10:{minute:02}:00.000Z"),
            )
            .unwrap();
        assert_eq!(made.kind, BackupKind::Manual);
        assert!(made.name.starts_with("manual-"));
    }
    let all = list(&folder);
    assert_eq!(
        all.iter().filter(|b| b.kind == BackupKind::Backup).count(),
        3
    );
    // ...and keeps only the newest manual ones.
    assert_eq!(
        all.iter().filter(|b| b.kind == BackupKind::Manual).count(),
        7
    );
    assert!(all.iter().all(|b| b.name != "manual-20261104-100000-000"));
    // Manual backups are not the daily one: it is due a day after the last daily backup,
    // however many manual ones exist.
    let at = |time| service.backup_if_due(&f.db, &f.paths, time).unwrap();
    assert!(at(datetime!(2026-11-04 09:30:00 UTC)).is_none());
    assert!(at(datetime!(2026-11-04 11:00:00 UTC)).is_some());
    // A manual backup can be restored like any other.
    restore(
        &f.db,
        &f.paths,
        "manual-20261104-100800-000",
        "2026-11-06T10:00:00.000Z",
    )
    .unwrap();
}

#[test]
fn a_backup_is_due_after_a_day_and_ignores_safety_backups_and_leftovers() {
    let f = fixture();
    let folder = default_folder(&f.paths);
    let service = BackupService::default();
    let at = |text: OffsetDateTime| service.backup_if_due(&f.db, &f.paths, text).unwrap();

    // None yet: one is made. Right after, nothing is due.
    assert!(at(datetime!(2026-11-19 10:00:00 UTC)).is_some());
    assert!(at(datetime!(2026-11-19 11:00:00 UTC)).is_none());
    assert!(at(datetime!(2026-11-20 09:59:59 UTC)).is_none());
    assert!(at(datetime!(2026-11-20 10:00:01 UTC)).is_some());
    assert_eq!(list(&folder).len(), 2);

    // A leftover temporary folder is not a backup, and is cleaned at the next backup.
    fs::create_dir_all(folder.join(".tmp-backup-20261201-000000-000")).unwrap();
    assert_eq!(list(&folder).len(), 2);
    assert!(at(datetime!(2026-11-22 10:00:00 UTC)).is_some());
    assert!(!folder.join(".tmp-backup-20261201-000000-000").exists());
}

#[test]
fn a_backup_dated_in_the_future_does_not_stop_new_ones() {
    let f = fixture();
    let service = BackupService::default();
    // The clock was wrong once and made a backup dated far ahead.
    backup(&f, "2030-01-01T10:00:00.000Z");
    let made = service
        .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 10:00:00 UTC))
        .unwrap();
    assert!(made.is_some());
}

#[test]
fn a_failed_backup_leaves_no_partial_folder_and_is_reported() {
    let f = fixture();
    let service = BackupService::default();
    // The backup folder is a file: nothing can be created in it.
    let blocker = f.paths.data_dir().join("blocked");
    fs::write(&blocker, b"file").unwrap();
    f.db.with(|conn| set_setting(conn, SettingKey::BackupFolder, &blocker.to_str().unwrap()))
        .unwrap();

    assert!(
        service
            .backup_now(&f.db, &f.paths, "2026-11-19T10:00:00.000Z")
            .is_err()
    );
    let status = service.status(&f.db, &f.paths).unwrap();
    assert!(status.last_error.is_some());

    f.db.with(|conn| {
        set_setting(
            conn,
            SettingKey::BackupFolder,
            &f.paths.data_dir().join("ok").to_str().unwrap(),
        )
    })
    .unwrap();
    service
        .backup_now(&f.db, &f.paths, "2026-11-19T10:00:00.000Z")
        .unwrap();
    assert_eq!(service.status(&f.db, &f.paths).unwrap().last_error, None);
}

#[test]
fn a_logged_delete_removes_the_tree_and_tolerates_a_missing_one() {
    let f = fixture();
    let tree = f.paths.data_dir().join("tree");
    fs::create_dir_all(tree.join("a")).unwrap();
    fs::write(tree.join("a").join("file"), b"x").unwrap();
    remove_dir_logged(&tree);
    assert!(!tree.exists());
    remove_dir_logged(&tree);
}

#[test]
fn computers_sharing_one_folder_rotate_only_their_own_backups() {
    let f = fixture();
    let folder = default_folder(&f.paths);
    as_computer("laptop");
    for day in 1..=3 {
        backup(&f, &format!("2026-11-{day:02}T09:00:00.000Z"));
    }
    // An unfinished backup of the laptop, and an old untagged one.
    fs::create_dir_all(folder.join(".tmp-backup-20261101-000000-000-laptop")).unwrap();
    fs::create_dir_all(folder.join(".tmp-backup-20261101-000000-000")).unwrap();
    as_computer("desktop");
    for day in 4..=12 {
        backup(&f, &format!("2026-11-{day:02}T10:00:00.000Z"));
    }
    let all = list(&folder);
    let by = |tag: &str| {
        all.iter()
            .filter(|b| b.computer.as_deref() == Some(tag))
            .count()
    };
    assert_eq!(by("desktop"), 7, "the desktop keeps its own newest 7");
    assert_eq!(by("laptop"), 3, "the laptop's backups are not touched");
    assert!(
        all.iter()
            .filter(|b| b.computer.as_deref() == Some("laptop"))
            .all(|b| b.other_computer)
    );
    assert!(
        all.iter()
            .filter(|b| b.computer.as_deref() == Some("desktop"))
            .all(|b| !b.other_computer)
    );
    assert!(
        folder
            .join(".tmp-backup-20261101-000000-000-laptop")
            .exists()
    );
    assert!(folder.join(".tmp-backup-20261101-000000-000").exists());
    assert_eq!(all[0].name, "backup-20261112-100000-000-desktop");
}

#[cfg(unix)]
#[test]
fn old_untagged_and_garbage_temporary_folders_are_cleaned_but_other_computers_are_not() {
    let f = fixture();
    let folder = default_folder(&f.paths);
    fs::create_dir_all(&folder).unwrap();
    let long_ago = std::time::SystemTime::now() - Duration::from_secs(3 * 24 * 60 * 60);
    let make = |name: &str, old: bool| {
        let path = folder.join(name);
        fs::create_dir_all(&path).unwrap();
        if old {
            fs::File::open(&path)
                .unwrap()
                .set_modified(long_ago)
                .unwrap();
        }
        path
    };
    let stale_untagged = make(".tmp-backup-20261101-000000-000", true);
    let stale_garbage = make(".tmp-whatever", true);
    let fresh_untagged = make(".tmp-backup-20261102-000000-000", false);
    let stale_other = make(".tmp-backup-20261101-000000-000-laptop", true);
    as_computer("desktop");
    backup(&f, "2026-11-19T10:00:00.000Z");
    assert!(!stale_untagged.exists());
    assert!(!stale_garbage.exists());
    assert!(fresh_untagged.exists(), "could be a build in progress");
    assert!(
        stale_other.exists(),
        "the other computer's is never touched"
    );
}

#[test]
fn untagged_backups_from_before_tags_are_kept_and_listed() {
    let f = fixture();
    let folder = default_folder(&f.paths);
    for day in 1..=8 {
        backup(&f, &format!("2026-11-{day:02}T10:00:00.000Z"));
    }
    assert_eq!(list(&folder).len(), 7, "no tag: rotation as before");
    as_computer("desktop");
    backup(&f, "2026-11-09T10:00:00.000Z");
    let all = list(&folder);
    assert_eq!(all.len(), 8, "the untagged ones stay");
    let old = all.iter().find(|b| b.computer.is_none()).unwrap();
    assert!(!old.other_computer);
}

#[test]
fn backups_with_an_old_style_tag_of_this_computer_are_still_its_own() {
    let f = fixture();
    let folder = default_folder(&f.paths);
    let service = BackupService::default();
    // Made by builds that slugged the host name the old way.
    as_computer("m-quina");
    for day in 1..=7 {
        backup(&f, &format!("2026-11-{day:02}T10:00:00.000Z"));
    }
    as_computer("laptop");
    backup(&f, "2026-11-08T10:00:00.000Z");
    // The tag derivation changed.
    as_computer("maquina");
    PREVIOUS.with(|tag| *tag.borrow_mut() = Some("m-quina".to_owned()));

    let all = list(&folder);
    let old_style = |b: &&BackupInfo| b.computer.as_deref() == Some("m-quina");
    assert_eq!(all.iter().filter(old_style).count(), 7);
    assert!(all.iter().filter(old_style).all(|b| !b.other_computer));
    assert!(
        all.iter()
            .filter(|b| b.computer.as_deref() == Some("laptop"))
            .all(|b| b.other_computer),
        "another computer's tag is never claimed"
    );

    // Its own, so they count for the daily check...
    assert!(
        service
            .backup_if_due(&f.db, &f.paths, datetime!(2026-11-07 20:00:00 UTC))
            .unwrap()
            .is_none(),
        "the old-style backup of that day counts"
    );
    // ... and they rotate together with the new ones: the oldest go first.
    backup(&f, "2026-11-09T10:00:00.000Z");
    let all = list(&folder);
    let own = all.iter().filter(|b| !b.other_computer).count();
    assert_eq!(own, 7, "seven kept in all, old-style and new");
    assert!(!all.iter().any(|b| b.name.starts_with("backup-20261101")));
    assert_eq!(all.iter().filter(|b| b.other_computer).count(), 1);
}

#[test]
fn another_computers_backup_does_not_make_this_one_not_due_and_can_be_restored() {
    let f = fixture();
    let folder = default_folder(&f.paths);
    as_computer("laptop");
    let theirs = backup(&f, "2026-11-19T10:00:00.000Z");
    as_computer("desktop");
    let service = BackupService::default();
    let made = service
        .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 11:00:00 UTC))
        .unwrap();
    assert_eq!(made.unwrap().computer.as_deref(), Some("desktop"));
    assert!(
        service
            .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 12:00:00 UTC))
            .unwrap()
            .is_none(),
        "its own backup now counts"
    );
    // The laptop's backup can be restored here; the safety backup is tagged with this
    // computer.
    restore(&f.db, &f.paths, &theirs.name, "2026-11-19T13:00:00.000Z").unwrap();
    assert!(folder.join("safety-20261119-130000-000-desktop").is_dir());
}

#[test]
fn the_daily_check_reads_names_not_manifests() {
    let f = fixture();
    let service = service_with_long_probe();
    let info = backup(&f, "2026-11-19T10:00:00.000Z");
    // A manifest that can't be read (a cloud-only placeholder, a half-synced copy) must not
    // make the backup look absent, nor slow the check down.
    let folder = default_folder(&f.paths);
    fs::remove_file(folder.join(&info.name).join(snapshot::MANIFEST_FILE)).unwrap();

    let made = service
        .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 11:00:00 UTC))
        .unwrap();
    assert!(made.is_none(), "the backup is there, by its name");
    let made = service
        .backup_if_due(&f.db, &f.paths, datetime!(2026-11-20 10:00:01 UTC))
        .unwrap();
    assert!(made.is_some(), "and a day later a new one is due");
}

#[test]
fn the_daily_backup_refuses_a_gone_drive_like_everything_else() {
    let f = fixture();
    let service = service_with_long_probe();
    f.db.with(|conn| {
        let gone = f._dir.path().join("volume-gone").join("backups");
        set_setting(conn, SettingKey::BackupFolder, &gone.display().to_string())
    })
    .unwrap();

    let error = service
        .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 10:00:00 UTC))
        .unwrap_err();

    assert!(error.to_string().contains("não está acessível"), "{error}");
    assert!(
        !f._dir.path().join("volume-gone").exists(),
        "nothing recreated"
    );
}

#[test]
fn untagged_backups_from_before_tags_still_count_for_the_daily_check() {
    let f = fixture();
    let service = service_with_long_probe();
    backup(&f, "2026-11-19T10:00:00.000Z");
    as_computer("desktop");

    let made = service
        .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 11:00:00 UTC))
        .unwrap();

    assert!(
        made.is_none(),
        "the untagged one is from before tags, this computer's"
    );
}
