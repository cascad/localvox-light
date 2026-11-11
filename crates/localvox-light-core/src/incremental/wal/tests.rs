use super::*;
fn key(n: u64) -> String {
    format!("{n:064x}")
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
