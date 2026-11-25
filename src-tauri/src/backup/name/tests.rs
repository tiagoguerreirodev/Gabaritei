use super::*;
use time::macros::datetime;

#[test]
fn backup_names_round_trip() {
    assert_eq!(
        Stamp::at(datetime!(2026-11-19 21:00:00.123 UTC)).as_str(),
        "20261119-210000-123"
    );
    assert_eq!(
        parse_name("backup-20261119-210000-123"),
        Some(BackupKind::Backup)
    );
    assert_eq!(
        parse_name("safety-20261119-210000-123"),
        Some(BackupKind::Safety)
    );
    assert_eq!(parse_name("backup-20261119-210000-12"), None);
    assert_eq!(parse_name(".tmp-backup-20261119-210000-123"), None);
}

#[test]
fn reads_the_computer_tag_from_a_name() {
    let parsed = BackupName::parse("manual-20261119-210000-123-desktop-a1").unwrap();
    assert_eq!(parsed.kind, BackupKind::Manual);
    assert_eq!(parsed.tag.as_deref(), Some("desktop-a1"));
}

#[test]
fn reads_old_untagged_names() {
    assert_eq!(
        BackupName::parse("backup-20261119-210000-123").unwrap().tag,
        None
    );
}

#[test]
fn rejects_invalid_computer_tags() {
    for bad in [
        "backup-20261119-210000-123-",
        "backup-20261119-210000-123--x",
        "backup-20261119-210000-123-X",
        "backup-20261119-210000-123-a-",
        "backup-20261119-210000-123x",
        "backup-20261119-210000-123-a_b",
        "backup-20261119-210000-123-abcdefghijklmnopqrstuvwxyz",
    ] {
        assert_eq!(BackupName::parse(bad), None, "{bad}");
    }
}

#[test]
fn a_stamp_reads_as_the_time_of_the_backup() {
    assert_eq!(
        BackupName::parse("backup-20261119-100203-045")
            .unwrap()
            .stamp,
        Stamp::at(datetime!(2026-11-19 10:02:03.045 UTC))
    );
}
