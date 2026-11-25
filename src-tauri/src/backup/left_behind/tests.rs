use super::*;
use crate::backup::fixtures::*;

#[cfg(unix)]
#[test]
fn a_link_named_like_a_backup_is_not_a_backup_for_the_notice() {
    let f = fixture();
    let service = BackupService::default();
    let (old, _) = make_left_behind(&f, &service);
    let real = list(&old).remove(0).name;
    std::os::unix::fs::symlink(old.join(&real), old.join("backup-20261101-000000-000")).unwrap();

    let notice = service
        .status(&f.db, &f.paths)
        .unwrap()
        .left_behind
        .unwrap();

    assert_eq!(notice.backups, 2, "the link is not counted");
}

#[test]
fn the_new_folder_is_created_when_the_move_starts() {
    let f = fixture();
    let service = BackupService::default();
    let (_, new) = make_left_behind(&f, &service);
    fs::remove_dir_all(&new).unwrap();

    let outcome = service
        .relocate_left_behind(&f.db, &f.paths, false)
        .unwrap();

    assert_eq!(outcome.report.copied, 2);
    assert_eq!(list(&new).len(), 2);
}

#[test]
fn dismissing_the_offer_between_two_backups_stops_the_move() {
    let f = fixture();
    let service = BackupService::default();
    let (old, _) = make_left_behind(&f, &service);
    let turns = std::cell::Cell::new(0);

    let outcome = service
        .relocate_with(&f.db, &f.paths, true, &|| {
            turns.set(turns.get() + 1);
            if turns.get() == 1 {
                service.forget_left_behind(&f.db).unwrap();
            }
        })
        .unwrap();

    assert_eq!((outcome.report.removed, outcome.report.failed), (1, 1));
    assert_eq!(list(&old).len(), 1);
    assert_eq!(outcome.status.left_behind, None);
}

#[test]
fn the_offer_to_move_survives_a_restart() {
    let f = fixture();
    let (old, new) = make_left_behind(&f, &BackupService::default());

    // A new service has no memory: everything comes from the stored setting.
    let restarted = BackupService::default();
    let notice = restarted.status(&f.db, &f.paths).unwrap().left_behind;
    assert_eq!(
        notice.map(|left| left.folder),
        Some(old.display().to_string())
    );

    let outcome = restarted
        .relocate_left_behind(&f.db, &f.paths, true)
        .unwrap();
    assert_eq!(outcome.report.removed, 2);
    assert_eq!(list(&new).len(), 2);
    assert_eq!(restarted.status(&f.db, &f.paths).unwrap().left_behind, None);
    assert!(list(&old).is_empty());
}

#[test]
fn an_unreadable_old_folder_is_offered_as_unavailable_and_can_be_dismissed() {
    let f = fixture();
    let service = BackupService::default();
    let (old, _) = make_left_behind(&f, &service);
    // Like an unplugged drive or an offline share: the folder can't be read for now.
    let away = f._dir.path().join("away");
    fs::rename(&old, &away).unwrap();

    let away_notice = service
        .status(&f.db, &f.paths)
        .unwrap()
        .left_behind
        .unwrap();
    assert!(away_notice.unavailable);
    assert_eq!(away_notice.folder, old.display().to_string());
    let error = service
        .relocate_left_behind(&f.db, &f.paths, true)
        .unwrap_err();
    assert!(error.to_string().contains("não está acessível"), "{error}");

    // The drive comes back: the same offer, now with its backups.
    fs::rename(&away, &old).unwrap();
    let back = service
        .status(&f.db, &f.paths)
        .unwrap()
        .left_behind
        .unwrap();
    assert_eq!((back.backups, back.unavailable), (2, false));

    // Gone for good (or never coming back): the user can still dismiss it.
    fs::rename(&old, &away).unwrap();
    assert!(
        service
            .status(&f.db, &f.paths)
            .unwrap()
            .left_behind
            .is_some()
    );
    service.forget_left_behind(&f.db).unwrap();
    assert_eq!(service.status(&f.db, &f.paths).unwrap().left_behind, None);
}

#[test]
fn a_backup_with_a_broken_manifest_is_counted_fails_to_move_and_keeps_the_offer() {
    let f = fixture();
    let service = BackupService::default();
    let (old, _) = make_left_behind(&f, &service);
    // The status and the move both read names, so a half-synced backup (no manifest) is
    // not silently dropped from the count and then forgotten.
    let broken = list(&old).remove(0).name;
    fs::remove_file(old.join(&broken).join(snapshot::MANIFEST_FILE)).unwrap();
    // A plain file named like a backup is not a backup.
    fs::write(old.join("backup-20261101-000000-000"), b"x").unwrap();

    let notice = service
        .status(&f.db, &f.paths)
        .unwrap()
        .left_behind
        .unwrap();
    assert_eq!(notice.backups, 2);

    let outcome = service.relocate_left_behind(&f.db, &f.paths, true).unwrap();
    assert_eq!((outcome.report.removed, outcome.report.failed), (1, 1));
    let left = outcome.status.left_behind.unwrap();
    assert_eq!(left.backups, 1, "the offer stays for the one that failed");
    assert!(old.join(&broken).is_dir(), "the original is untouched");
}

#[test]
fn forgetting_a_stale_offer_never_erases_a_newer_one() {
    let f = fixture();
    f.db.with(|conn| remember_old_folder(conn, Some(Path::new("/newer"))))
        .unwrap();
    forget_old_folder_if(&f.db, Path::new("/older")).unwrap();
    let stored =
        f.db.with(|conn| get_setting::<String>(conn, SettingKey::BackupLeftBehind))
            .unwrap();
    assert_eq!(stored.as_deref(), Some("/newer"));

    forget_old_folder_if(&f.db, Path::new("/newer")).unwrap();
    let stored =
        f.db.with(|conn| get_setting::<String>(conn, SettingKey::BackupLeftBehind))
            .unwrap();
    assert_eq!(stored, None);
}

#[test]
fn dismissing_the_offer_forgets_it_for_good() {
    let f = fixture();
    let service = BackupService::default();
    let (old, _) = make_left_behind(&f, &service);

    service.forget_left_behind(&f.db).unwrap();

    let restarted = BackupService::default();
    assert_eq!(restarted.status(&f.db, &f.paths).unwrap().left_behind, None);
    assert!(
        restarted
            .relocate_left_behind(&f.db, &f.paths, true)
            .is_err()
    );
    assert_eq!(list(&old).len(), 2, "nothing was deleted");
}

#[test]
fn the_offer_goes_away_once_the_old_folder_has_nothing_of_ours() {
    let f = fixture();
    let service = BackupService::default();
    let (old, _) = make_left_behind(&f, &service);
    for info in list(&old) {
        fs::remove_dir_all(old.join(info.name)).unwrap();
    }
    assert_eq!(service.status(&f.db, &f.paths).unwrap().left_behind, None);
    assert!(service.relocate_left_behind(&f.db, &f.paths, true).is_err());
}
