use super::*;

#[test]
fn backup_names_round_trip() {
    assert_eq!(stamp("2026-11-19T21:00:00.123Z"), "20261119-210000-123");
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
fn names_may_carry_a_computer_tag_and_old_untagged_names_still_read() {
    let parsed = parse("manual-20261119-210000-123-desktop-a1").unwrap();
    assert_eq!(parsed.kind, BackupKind::Manual);
    assert_eq!(parsed.tag.as_deref(), Some("desktop-a1"));
    assert_eq!(parse("backup-20261119-210000-123").unwrap().tag, None);
    for bad in [
        "backup-20261119-210000-123-",
        "backup-20261119-210000-123--x",
        "backup-20261119-210000-123-X",
        "backup-20261119-210000-123-a-",
        "backup-20261119-210000-123x",
        "backup-20261119-210000-123-a_b",
        "backup-20261119-210000-123-abcdefghijklmnopqrstuvwxyz",
    ] {
        assert_eq!(parse(bad), None, "{bad}");
    }
}

#[test]
fn a_stamp_reads_as_the_time_of_the_backup() {
    assert_eq!(
        stamp_to_iso("20261119-100203-045"),
        "2026-11-19T10:02:03.045Z"
    );
}
