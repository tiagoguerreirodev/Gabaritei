use super::*;
use crate::backup::fixtures::*;

#[test]
fn the_folders_to_open_are_the_data_folder_and_the_current_backup_folder() {
    let f = fixture();

    let data = folder_to_open(&f.db, &f.paths, OpenableFolder::Data).unwrap();
    assert_eq!(data, f.paths.data_dir());

    // The backup folder doesn't exist before the first backup: it is created, not an error.
    let default = default_folder(&f.paths);
    assert!(!default.exists());
    let backups = folder_to_open(&f.db, &f.paths, OpenableFolder::Backups).unwrap();
    assert_eq!(backups, default);
    assert!(default.is_dir());

    // After the user picks another folder, that is the one to open.
    let chosen = f._dir.path().join("elsewhere");
    BackupService::default()
        .change_folder(&f.db, &f.paths, Some(&chosen))
        .unwrap();
    let backups = folder_to_open(&f.db, &f.paths, OpenableFolder::Backups).unwrap();
    assert_eq!(backups, chosen);
}

#[test]
fn the_openable_folder_is_a_closed_choice_not_a_path() {
    assert_eq!(
        serde_json::from_str::<OpenableFolder>("\"backups\"").unwrap(),
        OpenableFolder::Backups
    );
    assert!(serde_json::from_str::<OpenableFolder>("\"/etc\"").is_err());
}

#[test]
fn the_folder_must_be_absolute_and_writable() {
    let f = fixture();
    let service = BackupService::default();
    let change = |chosen| service.change_folder(&f.db, &f.paths, chosen);
    assert!(change(Some(Path::new("relative/dir"))).is_err());
    assert!(change(Some(Path::new(""))).is_err());
    let images = f.paths.images_dir();
    assert!(change(Some(&images)).is_err());

    let file = f.paths.data_dir().join("file");
    fs::write(&file, []).unwrap();
    let error = change(Some(&file)).unwrap_err();
    assert!(error.to_string().contains("Não foi possível gravar"));

    let chosen = f.paths.data_dir().join("my backups");
    assert_eq!(
        change(Some(&chosen)).unwrap().folder,
        chosen.display().to_string()
    );
    assert!(chosen.is_dir());

    let status = change(None).unwrap();
    assert!(status.is_default_folder);
    assert_eq!(
        status.folder,
        default_folder(&f.paths).display().to_string()
    );
}

#[test]
fn the_folder_cannot_be_inside_a_backup_or_a_temporary_folder() {
    let f = fixture();
    let service = BackupService::default();
    let root = f.paths.data_dir().join("shared");
    for inside in [
        root.join("backup-20261119-210000-123").join("sub"),
        root.join("manual-20261119-210000-123-pc")
            .join("a")
            .join("b"),
        root.join("safety-20261119-210000-123"),
        root.join(".tmp-backup-20261119-210000-123"),
        root.join(".tmp-whatever").join("sub"),
    ] {
        let error = service
            .change_folder(&f.db, &f.paths, Some(&inside))
            .unwrap_err();
        assert!(error.to_string().contains("dentro de um backup"));
        assert!(
            !inside.exists(),
            "nothing was created: {}",
            inside.display()
        );
    }
    // A folder merely named like a backup prefix is fine.
    service
        .change_folder(&f.db, &f.paths, Some(&root.join("backups-do-pc")))
        .unwrap();
}

#[cfg(unix)]
#[test]
fn a_link_into_a_backup_does_not_get_past_the_folder_check() {
    let f = fixture();
    let service = BackupService::default();
    let root = f.paths.data_dir().join("shared");
    let real = root.join("backup-20261119-210000-123");
    fs::create_dir_all(&real).unwrap();
    let link = f.paths.data_dir().join("innocent");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    for inside in [&link, &link.join("sub"), &link.join("a").join("b")] {
        let error = service
            .change_folder(&f.db, &f.paths, Some(inside))
            .unwrap_err();
        assert!(error.to_string().contains("dentro de um backup"));
    }
    // Refusing must not leave empty folders inside the backup.
    assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn a_dangling_link_into_a_backup_creates_nothing_either() {
    let f = fixture();
    let service = BackupService::default();
    let root = f.paths.data_dir().join("shared");
    let real = root.join("backup-20261119-210000-123");
    fs::create_dir_all(&real).unwrap();
    // The link's target does not exist yet.
    let link = f.paths.data_dir().join("dangling");
    std::os::unix::fs::symlink(real.join("missing"), &link).unwrap();
    let error = service
        .change_folder(&f.db, &f.paths, Some(&link.join("sub")))
        .unwrap_err();
    assert!(error.to_string().contains("Não foi possível gravar"));
    assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
}

#[test]
fn a_new_folder_that_is_a_plain_subfolder_of_the_old_one_is_not_moved_into() {
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
    // Accepted as a backup folder, but a move would delete the copy
    // together with the original, so the relocation refuses it.
    service
        .change_folder(&f.db, &f.paths, Some(&old.join("nested")))
        .unwrap();

    assert!(service.relocate_left_behind(&f.db, &f.paths, true).is_err());
    snapshot::verify(&old.join(&info.name)).unwrap();
}

#[test]
fn a_backup_folder_that_does_not_exist_yet_is_reachable() {
    let f = fixture();
    let service = service_with_long_probe();
    // No backup was ever made: the default folder is missing, and that is fine.
    assert!(!default_folder(&f.paths).exists());
    assert_eq!(
        service.status(&f.db, &f.paths).unwrap().listing,
        BackupListing::Ready {
            backups: Vec::new()
        }
    );
    service
        .backup_now(&f.db, &f.paths, "2026-11-19T10:00:00.000Z")
        .unwrap();
    assert_eq!(list(&default_folder(&f.paths)).len(), 1);
}

#[test]
fn a_drive_that_is_gone_shows_as_unavailable_and_a_new_folder_does_not() {
    let f = fixture();
    let service = service_with_long_probe();
    let set = |folder: &Path| {
        f.db.with(|conn| {
            set_setting(
                conn,
                SettingKey::BackupFolder,
                &folder.display().to_string(),
            )
        })
        .unwrap();
    };
    // The parent (the volume) is gone: nothing to create the folder in.
    set(&f._dir.path().join("volume-gone").join("backups"));
    assert_eq!(
        service.status(&f.db, &f.paths).unwrap().listing,
        BackupListing::FolderUnavailable
    );
    assert!(
        service
            .backup_now(&f.db, &f.paths, "2026-11-19T10:00:00.000Z")
            .is_err()
    );

    // The parent exists and the folder is only missing: fine, created on first use.
    set(&f._dir.path().join("backups-novos"));
    assert_eq!(
        service.status(&f.db, &f.paths).unwrap().listing,
        BackupListing::Ready {
            backups: Vec::new()
        }
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_folder_on_a_volume_that_is_not_mounted_is_unavailable() {
    assert!(matches!(
        folder_state(Path::new("/Volumes/gabaritei-no-such-volume")),
        FolderState::Unavailable
    ));
}

#[cfg(windows)]
#[test]
fn a_drive_root_that_is_missing_is_unavailable_not_a_new_folder() {
    let letter = ('Q'..='Z')
        .find(|letter| !Path::new(&format!("{letter}:\\")).exists())
        .expect("an unused drive letter");
    assert!(matches!(
        folder_state(Path::new(&format!("{letter}:\\"))),
        FolderState::Unavailable
    ));
}
