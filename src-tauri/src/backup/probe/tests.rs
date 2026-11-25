use super::*;
use crate::backup::fixtures::*;

#[test]
fn the_probe_gives_up_on_work_that_never_answers() {
    // Only the give-up is timed, and it can't fail by being slow: the work never ends.
    let probe = FolderProbe::with_timeout(Duration::from_millis(50));
    let (release, work) = blocked();
    assert_eq!(probe.run(Path::new("/a"), work), None);
    // It is now marked stuck, and a later call on the same folder answers at once.
    assert!(
        lock(&probe.probes.0)
            .get(Path::new("/a"))
            .is_some_and(|probe| probe.stuck)
    );
    drop(release);
}

#[test]
fn a_stuck_folder_answers_at_once_and_recovers_when_its_work_ends() {
    let probe = FolderProbe::with_timeout(LONG);
    let path = Path::new("/a");
    assert_eq!(probe.run(path, || 1), Some(1));
    assert!(!occupied(&probe));

    probe.mark_stuck(path);
    // With a 60 s timeout, answering at all (instead of waiting) proves it is immediate.
    assert_eq!(probe.run(path, || 2), None);

    // A stuck entry is freed by its own worker; emulate that and check it works again.
    lock(&probe.probes.0).remove(path);
    assert_eq!(probe.run(path, || 3), Some(3));
}

#[test]
fn a_probe_stuck_on_one_folder_does_not_stop_another_folder() {
    let probe = FolderProbe::with_timeout(LONG);
    probe.mark_stuck(Path::new("/offline"));

    assert_eq!(probe.run(Path::new("/healthy"), || 5), Some(5));
    assert_eq!(probe.run(Path::new("/offline"), || 6), None);
}

#[test]
fn a_concurrent_call_waits_for_a_healthy_probe_and_then_runs() {
    let probe = Arc::new(FolderProbe::with_timeout(Duration::from_secs(10)));
    let (release, work) = blocked();
    let first = {
        let probe = probe.clone();
        std::thread::spawn(move || probe.run(Path::new("/a"), work))
    };
    eventually(|| occupied(&probe));
    let second = {
        let probe = probe.clone();
        std::thread::spawn(move || probe.run(Path::new("/a"), || 2))
    };
    // The second one is waiting; letting the first finish lets it through.
    std::thread::sleep(Duration::from_millis(50));
    drop(release);
    assert_eq!(first.join().unwrap(), Some(7));
    assert_eq!(second.join().unwrap(), Some(2));
}

#[test]
fn a_panicking_probe_does_not_disable_the_next_one() {
    let probe = FolderProbe::with_timeout(Duration::from_secs(5));
    let path = Path::new("/a");
    assert_eq!(probe.run(path, || -> u32 { panic!("boom") }), None);
    eventually(|| !occupied(&probe));
    assert_eq!(probe.run(path, || 4), Some(4));
}

#[test]
fn the_same_folder_is_answered_without_looking_at_the_disk() {
    let probe = FolderProbe::with_timeout(LONG);
    probe.mark_stuck(Path::new("/offline"));
    assert!(matches!(
        probe.inspect(Path::new("/offline"), Path::new("/offline")),
        Some(OldFolder::Current)
    ));
}

#[test]
fn a_slow_manifest_read_does_not_hold_the_folders_own_slot() {
    let service = BackupService {
        probe: FolderProbe::with_timeout(Duration::from_secs(5)),
        ..BackupService::default()
    };
    let folder = Path::new("/some/backups");
    let (release, work) = blocked();
    let key = manifest_key(folder);
    std::thread::scope(|scope| {
        let slow = scope.spawn(|| service.probe.run_for(&key, LONG, work));
        eventually(|| occupied(&service.probe));
        assert_eq!(service.probe.run(folder, || 1), Some(1));
        drop(release);
        assert_eq!(slow.join().unwrap(), Some(7));
    });
}
