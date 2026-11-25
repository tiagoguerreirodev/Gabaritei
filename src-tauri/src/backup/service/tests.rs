use super::*;
use crate::backup::fixtures::*;
use time::macros::datetime;

#[test]
fn old_style_backups_of_this_computer_are_moved_with_the_rest() {
    let f = fixture();
    let service = BackupService::default();
    let old = default_folder(&f.paths);
    as_computer("m-quina");
    backup(&f, "2026-11-19T10:00:00.000Z");
    as_computer("laptop");
    backup(&f, "2026-11-19T11:00:00.000Z");
    as_computer("maquina");
    PREVIOUS.with(|tag| *tag.borrow_mut() = Some("m-quina".to_owned()));
    let new = f._dir.path().join("new");
    service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();

    let outcome = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Move)
        .unwrap();

    assert_eq!((outcome.report.removed, outcome.report.left_alone), (1, 1));
    assert_eq!(list(&new).len(), 1);
    assert_eq!(list(&old).len(), 1, "only the laptop's stays");
}

#[test]
fn each_backup_is_moved_in_its_own_turn_and_a_refused_turn_stops_the_rest() {
    let f = fixture();
    let service = BackupService::default();
    let (old, new) = make_left_behind(&f, &service);
    let names: Vec<String> = list(&old).into_iter().map(|info| info.name).collect();
    assert_eq!(names.len(), 2);

    let turns = std::cell::Cell::new(0);
    let report = relocate::relocate(&old, &new, &names, Relocation::Move, &|work| {
        turns.set(turns.get() + 1);
        // The second turn is refused (say the folder changed meanwhile).
        if turns.get() == 1 {
            work();
        }
    })
    .unwrap();

    assert_eq!(turns.get(), 2, "one turn per backup, not one for all");
    assert_eq!((report.copied, report.removed, report.failed), (1, 1, 1));
    assert!(report.first_failure.is_some());
    assert_eq!(list(&old).len(), 1, "the refused one stays where it was");
}

#[test]
fn changing_the_folder_between_two_backups_stops_the_move_and_keeps_the_rest() {
    let f = fixture();
    let service = BackupService::default();
    let (old, new) = make_left_behind(&f, &service);
    let third = f._dir.path().join("third");
    let turns = std::cell::Cell::new(0);

    let outcome = service
        .relocate_with(&f.db, &f.paths, Relocation::Move, &|| {
            turns.set(turns.get() + 1);
            if turns.get() == 1 {
                // Between the first backup and the second.
                service
                    .change_folder(&f.db, &f.paths, Some(&third))
                    .unwrap();
            }
        })
        .unwrap();

    assert_eq!((outcome.report.removed, outcome.report.failed), (1, 1));
    assert_eq!(list(&old).len(), 1, "the second original was not touched");
    assert_eq!(list(&new).len(), 1);
    assert!(list(&third).is_empty());
    // The change made meanwhile decided what is offered: the old folder (`new` here) is
    // the one with the moved backup, and the notice was not overwritten by the move.
    let left = outcome.status.left_behind.map(|left| match left {
        LeftBehind::Available { folder, .. } | LeftBehind::Unavailable { folder } => folder,
    });
    assert_eq!(left, Some(new.display().to_string()));
}

#[test]
fn a_second_relocation_is_refused_while_one_runs_and_allowed_after_it() {
    let f = fixture();
    let service = BackupService::default();
    make_left_behind(&f, &service);
    let refused = std::cell::RefCell::new(None);

    let outcome = service
        .relocate_with(&f.db, &f.paths, Relocation::Move, &|| {
            if refused.borrow().is_none() {
                *refused.borrow_mut() =
                    Some(service.relocate_left_behind(&f.db, &f.paths, Relocation::Move));
            }
        })
        .unwrap();

    let refused = refused.into_inner().unwrap();
    assert!(
        refused
            .as_ref()
            .is_err_and(|e| e.to_string().contains("em andamento")),
        "{refused:?}"
    );
    assert_eq!((outcome.report.removed, outcome.report.failed), (2, 0));
    // The flag is released when the first one ends, even if it fails.
    assert!(!service.relocating.load(Ordering::Acquire));
    assert!(
        service
            .relocate_left_behind(&f.db, &f.paths, Relocation::Move)
            .is_err()
    );
    assert!(!service.relocating.load(Ordering::Acquire));
}

#[test]
fn a_missing_new_folder_inside_the_old_one_is_still_refused_and_not_created() {
    let f = fixture();
    let service = BackupService::default();
    let old = default_folder(&f.paths);
    backup(&f, "2026-11-19T10:00:00.000Z");
    let nested = old.join("sub").join("deeper");
    service
        .change_folder(&f.db, &f.paths, Some(&nested))
        .unwrap();
    fs::remove_dir_all(old.join("sub")).unwrap();

    assert!(
        service
            .relocate_left_behind(&f.db, &f.paths, Relocation::Move)
            .is_err()
    );

    assert!(
        !old.join("sub").exists(),
        "nothing was created inside the old folder"
    );
    assert_eq!(list(&old).len(), 1);
}

#[cfg(unix)]
#[test]
fn a_new_folder_behind_a_dangling_link_into_the_old_one_is_refused() {
    let f = fixture();
    let service = BackupService::default();
    let old = default_folder(&f.paths);
    backup(&f, "2026-11-19T10:00:00.000Z");
    // `link` points into the old folder at a place that does not exist yet.
    let link = f._dir.path().join("link");
    std::os::unix::fs::symlink(old.join("missing"), &link).unwrap();
    service
        .change_folder(&f.db, &f.paths, Some(&f._dir.path().join("elsewhere")))
        .unwrap();
    f.db.with(|conn| {
        // Chosen while the link still resolved to nothing; set directly, as a stale setting.
        set_setting(
            conn,
            SettingKey::BackupFolder,
            &link.join("sub").display().to_string(),
        )
    })
    .unwrap();

    let error = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Move)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("possível verificar a pasta nova"),
        "{error}"
    );

    assert!(
        !old.join("missing").exists(),
        "nothing was created through the link"
    );
    assert_eq!(list(&old).len(), 1);
}

#[test]
fn a_chosen_folder_that_does_not_answer_keeps_both_folder_settings() {
    let f = fixture();
    let service = service_with_long_probe();
    let (old, current) = make_left_behind(&f, &service);
    let chosen = f._dir.path().join("offline");
    service.probe.mark_stuck(&candidate_key(&chosen));

    let error = service
        .change_folder(&f.db, &f.paths, Some(&chosen))
        .unwrap_err();

    assert!(error.to_string().contains("não está acessível"), "{error}");
    let settings =
        f.db.with(|conn| {
            Ok((
                folder(conn, &f.paths)?,
                get_setting(conn, SettingKey::BackupLeftBehind)?,
            ))
        })
        .unwrap();
    assert_eq!(settings, (current, Some(old.display().to_string())));
}

#[test]
fn a_stalled_candidate_check_leaves_the_current_folder_readable() {
    let f = fixture();
    let mut service = service_with_long_probe();
    let folder = f._dir.path().join("chosen");
    service
        .change_folder(&f.db, &f.paths, Some(&folder))
        .unwrap();

    // Hold the write check past its deadline and until both read probes finish.
    let (release, work) = blocked();
    service.probe.timeout = Duration::from_millis(50);
    let error = service
        .change_folder_with(&f.db, &f.paths, Some(&folder), move |chosen, _| {
            work();
            Ok(chosen.display().to_string())
        })
        .unwrap_err();
    assert!(error.to_string().contains("não está acessível"), "{error}");
    service.probe.timeout = LONG;

    let status = service.status(&f.db, &f.paths).unwrap();
    assert_eq!(
        status.listing,
        BackupListing::Ready {
            backups: Vec::new()
        }
    );
    service
        .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 10:00:00 UTC))
        .unwrap()
        .expect("the daily backup is due");
    drop(release);
}

#[test]
fn a_backup_can_finish_while_the_chosen_folder_is_being_checked() {
    let f = fixture();
    let service = service_with_long_probe();
    let old = default_folder(&f.paths);
    let chosen = f._dir.path().join("chosen");
    let (f, service) = (&f, &service);
    std::thread::scope(|scope| {
        let (checking, started) = mpsc::channel();
        let (release, wait) = mpsc::channel::<()>();
        let change = scope.spawn(|| {
            service.change_folder_with(&f.db, &f.paths, Some(&chosen), move |chosen, images| {
                checking.send(()).unwrap();
                let _ = wait.recv();
                check_chosen_folder(chosen, images)
            })
        });
        started.recv_timeout(Duration::from_secs(10)).unwrap();

        let (done, backup) = mpsc::channel();
        scope.spawn(move || {
            let result = service.backup_now(&f.db, &f.paths, "2026-11-19T10:00:00.000Z");
            let _ = done.send(result);
        });
        backup
            .recv_timeout(Duration::from_secs(10))
            .expect("the backup can acquire both locks")
            .unwrap();
        release.send(()).unwrap();

        let status = change.join().unwrap().unwrap();
        let left = status.left_behind.unwrap();
        assert!(
            matches!(left, LeftBehind::Available { folder, backups: 1, .. } if folder == old.display().to_string())
        );
    });
}

#[test]
fn an_old_folder_that_does_not_answer_is_offered_as_unavailable_without_blocking() {
    let f = fixture();
    let (old, new) = make_left_behind(&f, &BackupService::default());
    let service = service_with_long_probe();
    // Reading the old folder is stuck.
    service.probe.mark_stuck(&old);

    let notice = service
        .status(&f.db, &f.paths)
        .unwrap()
        .left_behind
        .unwrap();
    assert_eq!(
        notice,
        LeftBehind::Unavailable {
            folder: old.display().to_string()
        }
    );

    // Changing the folder while the folder being left (`new` now) doesn't answer keeps the
    // offer for it (as unavailable) instead of losing it.
    service.probe.mark_stuck(&new);
    let third = f._dir.path().join("third");
    let status = service
        .change_folder(&f.db, &f.paths, Some(&third))
        .unwrap();
    let left = status.left_behind.unwrap();
    assert_eq!(
        left,
        LeftBehind::Unavailable {
            folder: new.display().to_string()
        }
    );
    let error = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Move)
        .unwrap_err();
    assert!(error.to_string().contains("não está acessível"), "{error}");
}

#[test]
fn a_folder_that_stops_answering_after_a_move_keeps_the_offer() {
    let f = fixture();
    let service = service_with_long_probe();
    let (old, _) = make_left_behind(&f, &service);
    // Leave one backup the move can't verify, so something remains, then make the folder
    // stop answering just before the final check.
    let broken = list(&old).remove(0).name;
    fs::remove_file(old.join(&broken).join(snapshot::MANIFEST_FILE)).unwrap();

    let outcome = service
        .relocate_with(&f.db, &f.paths, Relocation::Move, &|| {
            service.probe.mark_stuck(&old)
        })
        .unwrap();

    assert_eq!(outcome.report.failed, 1);
    let left = outcome.status.left_behind.expect("the offer stays");
    assert!(matches!(left, LeftBehind::Unavailable { .. }));
}

#[test]
fn a_current_folder_that_does_not_answer_shows_as_unavailable_and_blocks_nothing() {
    let f = fixture();
    let service = service_with_long_probe();
    let folder = default_folder(&f.paths);
    let made = backup(&f, "2026-11-19T10:00:00.000Z");
    let healthy = service.status(&f.db, &f.paths).unwrap();
    assert_eq!(
        healthy.listing,
        BackupListing::Ready {
            backups: vec![made]
        }
    );

    service.probe.mark_stuck(&folder);

    let status = service.status(&f.db, &f.paths).unwrap();
    assert_eq!(status.listing, BackupListing::FolderUnavailable);
    // Nothing that takes the backup lock waits on the folder: each is refused at once.
    let now = "2026-11-19T12:00:00.000Z";
    let errors = [
        service.backup_now(&f.db, &f.paths, now).unwrap_err(),
        service
            .backup_if_due(&f.db, &f.paths, datetime!(2026-11-21 10:00:00 UTC))
            .unwrap_err(),
        service
            .restore(
                &f.db,
                &f.paths,
                "backup-20261119-100000-000",
                now,
                || Ok(()),
            )
            .unwrap_err(),
        service
            .import(&f.db, &f.paths, Path::new("/nowhere.zip"), now, || Ok(()))
            .unwrap_err(),
    ];
    for error in errors {
        assert!(error.to_string().contains("não está acessível"), "{error}");
    }
    assert!(
        service
            .status(&f.db, &f.paths)
            .unwrap()
            .last_error
            .is_some()
    );
    assert_eq!(list(&folder).len(), 1, "nothing was written or removed");
}

#[test]
fn a_new_folder_that_does_not_answer_stops_the_relocation_before_it_creates_anything() {
    let f = fixture();
    let service = service_with_long_probe();
    let (old, new) = make_left_behind(&f, &service);
    service.probe.mark_stuck(&new);

    let error = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Move)
        .unwrap_err();

    assert!(error.to_string().contains("não está acessível"), "{error}");
    assert_eq!(list(&old).len(), 2, "nothing was touched");
}

#[test]
fn stuck_manifests_report_a_slow_list_while_backups_keep_working() {
    let f = fixture();
    let service = service_with_long_probe();
    let folder = default_folder(&f.paths);
    fs::create_dir_all(&folder).unwrap();
    // An earlier manifest read has already reached its deadline.
    service.probe.mark_stuck(&manifest_key(&folder));
    assert_eq!(
        service.status(&f.db, &f.paths).unwrap().listing,
        BackupListing::Slow
    );
    let daily = service
        .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 10:00:00 UTC))
        .unwrap()
        .expect("a daily backup was due");
    let manual = service
        .backup_now(&f.db, &f.paths, "2026-11-19T11:00:00.000Z")
        .unwrap();
    assert_eq!(
        service.status(&f.db, &f.paths).unwrap().listing,
        BackupListing::Slow
    );
    for info in [daily, manual] {
        snapshot::verify(&folder.join(info.name)).unwrap();
    }
}

#[test]
fn a_copy_that_failed_for_some_backups_can_be_tried_again() {
    let f = fixture();
    let service = BackupService::default();
    let (old, new) = make_left_behind(&f, &service);
    // Something unusable already sits under the name of one backup in the new folder.
    let blocked = list(&old)[0].name.clone();
    fs::create_dir_all(new.join(&blocked)).unwrap();

    let first = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Copy)
        .unwrap();
    assert_eq!((first.report.copied, first.report.failed), (1, 1));
    assert!(
        first.status.left_behind.is_some(),
        "the offer stays for the retry"
    );

    fs::remove_dir_all(new.join(&blocked)).unwrap();
    let second = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Copy)
        .unwrap();
    assert_eq!(second.report.failed, 0);
    assert_eq!(second.status.left_behind, None, "done: nothing failed");
    assert_eq!(list(&new).len(), 2);
}

#[test]
fn copying_the_left_behind_backups_keeps_the_originals() {
    let f = fixture();
    let service = BackupService::default();
    let (old, new) = make_left_behind(&f, &service);

    let outcome = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Copy)
        .unwrap();

    assert_eq!(outcome.report.copied, 2);
    assert_eq!(outcome.report.removed, 0);
    assert_eq!(outcome.report.failed, 0);
    assert_eq!(list(&old).len(), 2, "copying never deletes");
    assert_eq!(list(&new).len(), 2);
    assert_eq!(
        outcome.status.listing,
        BackupListing::Ready {
            backups: list(&new)
        }
    );
    assert_eq!(outcome.status.left_behind, None);
    // Verified copies, restorable from the new folder.
    for info in list(&new) {
        snapshot::verify(&new.join(&info.name)).unwrap();
    }
    // The notice is gone, so there is nothing more to move.
    assert!(
        service
            .relocate_left_behind(&f.db, &f.paths, Relocation::Move)
            .is_err()
    );
}

#[test]
fn moving_deletes_each_original_only_after_its_copy_is_verified() {
    let f = fixture();
    let service = BackupService::default();
    let (old, new) = make_left_behind(&f, &service);

    let outcome = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Move)
        .unwrap();

    assert_eq!(outcome.report.copied, 2);
    assert_eq!(outcome.report.removed, 2);
    assert!(list(&old).is_empty());
    assert_eq!(list(&new).len(), 2);
    assert_eq!(outcome.status.left_behind, None);
}

#[test]
fn a_backup_that_does_not_verify_after_copying_keeps_its_original() {
    let f = fixture();
    let service = BackupService::default();
    let (old, new) = make_left_behind(&f, &service);
    // Corrupt one original's database: its copy can't pass verification.
    let bad = list(&old)
        .into_iter()
        .find(|b| b.kind == BackupKind::Safety)
        .unwrap();
    fs::write(
        old.join(&bad.name).join("gabaritei.sqlite"),
        b"not a database",
    )
    .unwrap();

    let outcome = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Move)
        .unwrap();

    assert_eq!(outcome.report.copied, 1);
    assert_eq!(outcome.report.removed, 1);
    assert_eq!(outcome.report.failed, 1);
    assert!(old.join(&bad.name).is_dir(), "the original stays");
    assert!(!new.join(&bad.name).exists(), "no partial copy is left");
    assert!(
        !new.read_dir().unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".tmp-")),
        "no temporary folder is left"
    );
    // What is still in the old folder is reported, and can be tried again.
    assert_eq!(
        outcome.status.left_behind,
        Some(LeftBehind::Available {
            folder: old.display().to_string(),
            backups: 1,
            safety: 1,
        })
    );
}

#[test]
fn a_backup_already_in_the_new_folder_is_not_copied_again_but_can_be_removed() {
    let f = fixture();
    let service = BackupService::default();
    let (old, new) = make_left_behind(&f, &service);
    service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Copy)
        .unwrap();
    // The copy exists and the notice is gone: go to the default folder and back to get the
    // notice for the originals again.
    service.change_folder(&f.db, &f.paths, None).unwrap();
    service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();
    let outcome = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Move)
        .unwrap();
    assert_eq!(outcome.report.copied, 0);
    assert_eq!(outcome.report.already_there, 2);
    assert_eq!(outcome.report.removed, 2);
    assert!(list(&old).is_empty());
    assert_eq!(list(&new).len(), 2);
}

#[test]
fn a_different_backup_with_the_same_name_is_never_overwritten_or_removed() {
    let f = fixture();
    let service = BackupService::default();
    let old = default_folder(&f.paths);
    create(
        &f.db,
        &f.paths,
        &old,
        BackupKind::Backup,
        "2026-11-19T10:00:00.000Z",
        false,
    )
    .unwrap();
    let new = f._dir.path().join("synced");
    // Same name, different content (another computer without a tag, say).
    add_question(&f.db, "q-extra");
    create(
        &f.db,
        &f.paths,
        &new,
        BackupKind::Backup,
        "2026-11-19T10:00:00.000Z",
        false,
    )
    .unwrap();
    service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();

    let outcome = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Move)
        .unwrap();

    assert_eq!(outcome.report.failed, 1);
    assert_eq!(outcome.report.removed, 0);
    assert_eq!(list(&old).len(), 1);
    snapshot::verify(&new.join("backup-20261119-100000-000")).unwrap();
}

#[cfg(unix)]
#[test]
fn moving_into_the_same_physical_folder_never_deletes_the_only_copy() {
    let f = fixture();
    let old = default_folder(&f.paths);
    let info = create(
        &f.db,
        &f.paths,
        &old,
        BackupKind::Backup,
        "2026-11-19T10:00:00.000Z",
        false,
    )
    .unwrap();
    // Another spelling of the same folder.
    let alias = f._dir.path().join("alias");
    std::os::unix::fs::symlink(&old, &alias).unwrap();

    let report = relocate::relocate(
        &old,
        &alias,
        std::slice::from_ref(&info.name),
        Relocation::Move,
        &|work| work(),
    )
    .unwrap();

    assert_eq!(report.failed, 1);
    assert_eq!(report.removed, 0);
    snapshot::verify(&old.join(&info.name)).unwrap();
}

#[test]
fn a_new_folder_inside_the_old_one_is_refused() {
    let f = fixture();
    let service = BackupService::default();
    let old = default_folder(&f.paths);
    let info = create(
        &f.db,
        &f.paths,
        &old,
        BackupKind::Backup,
        "2026-11-19T10:00:00.000Z",
        false,
    )
    .unwrap();
    let inside = old.join(&info.name).join("sub");
    // Refused up front by the folder check (#109), so nothing is left to move into it.
    assert!(
        service
            .change_folder(&f.db, &f.paths, Some(&inside))
            .is_err()
    );
    assert!(!inside.exists());
    snapshot::verify(&old.join(&info.name)).unwrap();
}

#[test]
fn another_computers_backups_are_left_where_they_are() {
    let f = fixture();
    let service = BackupService::default();
    let old = default_folder(&f.paths);
    as_computer("laptop");
    let theirs = create(
        &f.db,
        &f.paths,
        &old,
        BackupKind::Backup,
        "2026-11-19T09:00:00.000Z",
        false,
    )
    .unwrap();
    as_computer("desktop");
    create(
        &f.db,
        &f.paths,
        &old,
        BackupKind::Backup,
        "2026-11-19T10:00:00.000Z",
        false,
    )
    .unwrap();
    let new = f._dir.path().join("synced");
    service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();

    let outcome = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Move)
        .unwrap();

    assert_eq!(outcome.report.removed, 1);
    assert_eq!(outcome.report.left_alone, 1);
    assert!(old.join(&theirs.name).is_dir(), "the laptop's backup stays");
    assert!(!new.join(&theirs.name).exists());
    assert_eq!(outcome.status.left_behind, None);
}

#[test]
fn untagged_backups_are_not_ours_to_move_on_a_tagged_computer() {
    let f = fixture();
    let service = BackupService::default();
    let old = default_folder(&f.paths);
    // Made before tags existed (or by a computer without a tag): it can't be told whose.
    TAG.with(|tag| *tag.borrow_mut() = Some(None));
    let untagged = create(
        &f.db,
        &f.paths,
        &old,
        BackupKind::Backup,
        "2026-11-19T09:00:00.000Z",
        false,
    )
    .unwrap();
    as_computer("desktop");
    create(
        &f.db,
        &f.paths,
        &old,
        BackupKind::Backup,
        "2026-11-19T10:00:00.000Z",
        false,
    )
    .unwrap();
    let new = f._dir.path().join("synced");
    service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();

    let outcome = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Move)
        .unwrap();

    assert_eq!(outcome.report.removed, 1);
    assert_eq!(outcome.report.left_alone, 1);
    assert!(
        old.join(&untagged.name).is_dir(),
        "never deleted from the old folder"
    );
    assert!(!new.join(&untagged.name).exists(), "and not copied either");
    assert_eq!(outcome.status.left_behind, None);
}

#[cfg(unix)]
#[test]
fn a_link_inside_a_backup_is_never_followed_and_extra_files_are_not_copied() {
    let f = fixture();
    let service = BackupService::default();
    let old = default_folder(&f.paths);
    let info = create(
        &f.db,
        &f.paths,
        &old,
        BackupKind::Backup,
        "2026-11-19T10:00:00.000Z",
        false,
    )
    .unwrap();
    let dir = old.join(&info.name);
    fs::write(dir.join("notes.txt"), b"extra").unwrap();
    let new = f._dir.path().join("synced");
    service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();
    let outcome = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Copy)
        .unwrap();
    assert_eq!(outcome.report.copied, 1);
    assert!(!new.join(&info.name).join("notes.txt").exists());

    // Now the database file is a link: not a regular file, so the copy is refused.
    let other = create(
        &f.db,
        &f.paths,
        &old,
        BackupKind::Backup,
        "2026-11-19T11:00:00.000Z",
        false,
    )
    .unwrap();
    let db_file = old.join(&other.name).join("gabaritei.sqlite");
    let real = old.join("real.sqlite");
    fs::rename(&db_file, &real).unwrap();
    std::os::unix::fs::symlink(&real, &db_file).unwrap();
    service.change_folder(&f.db, &f.paths, None).unwrap();
    service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();
    let outcome = service
        .relocate_left_behind(&f.db, &f.paths, Relocation::Move)
        .unwrap();
    assert_eq!(outcome.report.failed, 1);
    assert!(old.join(&other.name).is_dir());
}
