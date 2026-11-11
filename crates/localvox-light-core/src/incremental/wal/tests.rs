use super::*;
fn key(n: u64) -> String {
    format!("{n:064x}")
}

#[test]
fn grouped_frames_are_durable_and_failed_sync_is_not_an_acceptance() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("results");
    let mut store = Store::open(root.clone()).unwrap();
    store.put(&key(0), b"already durable").unwrap();
    let keys: Vec<_> = (1..=8).map(key).collect();
    let entries: Vec<_> = keys
        .iter()
        .map(|k| (k.as_str(), b"new result".as_slice()))
        .collect();
    assert!(store
        .put_many_with_sync(&entries, |_| Err(std::io::Error::other("refused sync")))
        .is_err());
    assert_eq!(store.get(&key(0)).unwrap().unwrap(), b"already durable");
    assert!(store.get(&key(1)).unwrap().is_none());
    let mut recovered = Store::open(root).unwrap();
    assert!(recovered
        .ensure_durable_with_sync(&key(1), |_| Err(std::io::Error::other(
            "refused recovery sync"
        )))
        .is_err());
    assert!(!recovered.durable_segments.contains(&0));
    recovered.ensure_durable(&key(1)).unwrap();
    let before = recovered.stats().syncs;
    recovered.ensure_durable(&key(2)).unwrap();
    assert_eq!(
        recovered.stats().syncs,
        before,
        "one recovery sync covers the segment"
    );
    recovered.put_many(&entries).unwrap();
    assert_eq!(recovered.stats().frames, 8);
    assert_eq!(
        recovered.stats().syncs,
        before + 1,
        "one sync covers all eight frames"
    );
    drop(recovered);
    let mut reopened = Store::open(dir.path().join("results")).unwrap();
    for k in keys {
        reopened.ensure_durable(&k).unwrap();
        assert_eq!(reopened.get(&k).unwrap().unwrap(), b"new result");
    }
}

#[test]
#[ignore = "explicit small disk journal timing probe; no models/devices"]
fn profile_result_journal_sync_cost() {
    let root = std::env::var_os("LOCALVOX_WAL_PROFILE_ROOT")
        .map(PathBuf::from)
        .expect("select diagnostic output root");
    let payload = serde_json::to_vec(&serde_json::json!({"text":"x".repeat(512)})).unwrap();
    for batch_size in [1, 8] {
        let dir = tempfile::tempdir_in(&root).unwrap();
        let mut store = Store::open(dir.path().join("results")).unwrap();
        let keys: Vec<_> = (0..1608).map(key).collect();
        let started = std::time::Instant::now();
        for group in keys.chunks(batch_size) {
            let entries: Vec<_> = group
                .iter()
                .map(|key| (key.as_str(), payload.as_slice()))
                .collect();
            store.put_many(&entries).unwrap();
            for key in group {
                assert_eq!(store.get(key).unwrap().unwrap(), payload);
            }
        }
        println!(
            "journal_probe={}",
            serde_json::json!({"batch_size":batch_size,"wall_sec":started.elapsed().as_secs_f64(),"writes":store.stats()})
        );
    }
}

#[test]
fn durable_frames_reopen_resync_corruption_and_ignore_an_incomplete_tail() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("results")).unwrap();
    store.put(&key(1), b"first").unwrap();
    store.put(&key(2), b"second").unwrap();
    store.put(&key(3), b"third").unwrap();
    let path = store.location(&key(1));
    let mut bytes = fs::read(&path).unwrap();
    bytes[HEADER] ^= 1; // Only the first frame is damaged; later frames remain reusable.
    let incomplete = frame(&key(4), b"unfinished payload").unwrap();
    bytes.extend_from_slice(&incomplete[..HEADER + 3]);
    fs::write(&path, &bytes).unwrap();
    let mut reopened = Store::open(dir.path().join("results")).unwrap();
    assert!(reopened.get(&key(1)).unwrap().is_none());
    assert!(reopened.get(&key(4)).unwrap().is_none());
    assert_eq!(reopened.get(&key(2)).unwrap().unwrap(), b"second");
    assert_eq!(reopened.get(&key(3)).unwrap().unwrap(), b"third");
    reopened.put(&key(1), b"repaired").unwrap();
    reopened.refresh().unwrap();
    assert_eq!(reopened.get(&key(1)).unwrap().unwrap(), b"repaired");
    let mut bytes = fs::read(&path).unwrap();
    bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    let checksum = Sha256::digest(&bytes[..76]);
    bytes[76..108].copy_from_slice(&checksum);
    fs::write(&path, bytes).unwrap();
    reopened.refresh().unwrap();
    assert_eq!(reopened.get(&key(2)).unwrap().unwrap(), b"second");
}

#[test]
fn segments_are_bounded_and_write_refusal_cannot_publish_an_entry() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("results")).unwrap();
    let file = store.path(0);
    fs::create_dir(&file).unwrap();
    assert!(store.put(&key(1), b"refused").is_err());
    assert!(store.get(&key(1)).unwrap().is_none());
    fs::remove_dir(&file).unwrap();
    let large = vec![b'x'; MAX_RECORD];
    store.put(&key(1), &large).unwrap();
    store.put(&key(2), &large).unwrap();
    assert_ne!(store.location(&key(1)), store.location(&key(2)));
    assert!(fs::metadata(store.location(&key(1))).unwrap().len() <= MAX_SEGMENT);
    assert!(store.put(&key(3), &vec![b'x'; MAX_RECORD + 1]).is_err());
    assert!(store.put(&key(3), b"not\0json").is_err());
    store.refresh().unwrap();
    assert_eq!(store.get(&key(2)).unwrap().unwrap(), large);
}
