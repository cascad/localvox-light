use super::*;
use crate::incremental::test_support::Fixture;

#[test]
fn restored_checkpoint_resyncs_a_repaired_orphan_even_when_its_digest_matches() {
    let f = Fixture::new([&[10, 20], &[10, 20]], true);
    let _lease = f.lease();
    let cache = Cache::open(&f.session, &f.identity).unwrap();
    let mut all = Vec::new();
    for id in [0, 1] {
        for n in 0..2 {
            all.push(
                cache
                    .resolve(f.window(id, n), |_| Ok("слова".into()))
                    .unwrap()
                    .0,
            );
        }
    }
    cache
        .begin(&f.horizon)
        .unwrap()
        .finish(&f.session, &all)
        .unwrap();
    let key = all[0].key.clone();
    let file = cache.wal.lock().unwrap().location(&key);
    cache
        .wal
        .lock()
        .unwrap()
        .replace(&key, b"persistently broken result")
        .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(file)
        .unwrap()
        .sync_all()
        .unwrap();
    let prepared = cache
        .prepare(f.window(0, 0), |_| Ok("слова".into()))
        .unwrap();
    let PreparedValue::New { key, bytes, .. } = prepared.0 else {
        panic!("corrupt record was reused")
    };
    assert!(cache
        .wal
        .lock()
        .unwrap()
        .put_refusing_sync(&[(key.as_str(), bytes.as_slice())])
        .is_err());
    drop(cache);
    let recovered = Cache::open(&f.session, &f.identity).unwrap();
    assert_eq!(recovered.wal.lock().unwrap().stats().syncs, 0);
    let state = recovered.begin(&f.horizon).unwrap();
    assert!(state.state().complete);
    assert_eq!(
        recovered.wal.lock().unwrap().stats().syncs,
        1,
        "matching checkpoint hashes do not replace a durability confirmation"
    );
}

#[test]
fn result_group_is_bounded_and_only_the_last_pending_tail_is_lost_on_drop() {
    let f = Fixture::new([&[10, 20, 30, 40, 50, 60, 70, 80, 90], &[]], true);
    let _lease = f.lease();
    let cache = Cache::open(&f.session, &f.identity).unwrap();
    let mut batch = crate::incremental::ResultBatch::new(&cache);
    for n in 0..8 {
        let result = batch
            .resolve(f.window(0, n), |_| Ok("слова".into()))
            .unwrap();
        assert_eq!(result.durable, n == 7);
    }
    assert_eq!(cache.wal.lock().unwrap().stats().syncs, 1);
    batch
        .resolve(f.window(0, 8), |_| Ok("хвост".into()))
        .unwrap();
    drop(batch);
    for n in 0..8 {
        assert!(cache.lookup(&f.window(0, n)).unwrap().is_some());
    }
    assert!(cache.lookup(&f.window(0, 8)).unwrap().is_none());
    assert!(!cache.root.join("checkpoint.json").exists());
}

#[test]
fn computed_group_is_not_reusable_until_flushed_and_a_drop_cannot_publish_it() {
    let f = Fixture::new([&[10, 20], &[10, 20]], true);
    let _lease = f.lease();
    let cache = Cache::open(&f.session, &f.identity).unwrap();
    {
        let mut batch = crate::incremental::ResultBatch::new(&cache);
        assert!(
            !batch
                .resolve(f.window(0, 0), |_| Ok("слова".into()))
                .unwrap()
                .durable
        );
        assert!(cache.lookup(&f.window(0, 0)).unwrap().is_none());
        assert!(
            batch
                .resolve(f.window(0, 0), |_| panic!("pending ASR repeated"))
                .unwrap()
                .reused
        );
    }
    assert!(cache.lookup(&f.window(0, 0)).unwrap().is_none());
    assert!(!f.session.join("incremental/current.json").exists());
    let transaction = cache.begin(&f.horizon).unwrap();
    let mut batch = crate::incremental::ResultBatch::new(&cache);
    for id in [0, 1] {
        for n in 0..2 {
            batch
                .resolve(f.window(id, n), |_| Ok("слова".into()))
                .unwrap();
        }
    }
    let accepted = batch.finish().unwrap();
    assert_eq!(accepted.len(), 4);
    assert_eq!(cache.wal.lock().unwrap().stats().syncs, 1);
    assert!(transaction.finish(&f.session, &accepted).unwrap().complete);
}

#[test]
fn refused_group_write_does_not_release_acceptance_or_advance_checkpoint() {
    let f = Fixture::new([&[10, 20], &[10, 20]], true);
    let _lease = f.lease();
    let cache = Cache::open(&f.session, &f.identity).unwrap();
    let transaction = cache.begin(&f.horizon).unwrap();
    let mut batch = crate::incremental::ResultBatch::new(&cache);
    batch
        .resolve(f.window(0, 0), |_| Ok("слова".into()))
        .unwrap();
    let journal = cache.root.join("results");
    fs::remove_dir(&journal).unwrap();
    fs::write(&journal, b"refuse journal creation").unwrap();
    assert!(batch.finish().is_err());
    assert_eq!(transaction.state().common_end, 0);
    assert!(!cache.root.join("checkpoint.json").exists());
    assert!(!f.session.join("incremental/current.json").exists());
    assert!(!f.window(0, 0).pcm().is_empty());
}

#[test]
fn transaction_revalidates_new_records_and_recovers_from_disk_on_the_next_request() {
    let f = Fixture::new([&[10, 20], &[10, 20]], true);
    let _lease = f.lease();
    let cache = Cache::open(&f.session, &f.identity).unwrap();
    let transaction = cache.begin(&f.horizon).unwrap();
    let accepted = cache
        .resolve(f.window(0, 0), |_| Ok("слова".into()))
        .unwrap()
        .0;
    let key = accepted.key.clone();
    cache
        .wal
        .lock()
        .unwrap()
        .replace(&key, b"broken after result")
        .unwrap();
    assert!(transaction.finish(&f.session, &[accepted]).is_err());
    assert!(!f.session.join("incremental/current.json").exists());
    let transaction = cache.begin(&f.horizon).unwrap();
    let mut all = Vec::new();
    for id in [0, 1] {
        for n in 0..2 {
            all.push(
                cache
                    .resolve(f.window(id, n), |_| Ok("слова".into()))
                    .unwrap()
                    .0,
            );
        }
    }
    let state = transaction.finish(&f.session, &all).unwrap();
    assert!(state.complete);
    assert!(crate::incremental::Published::current(&f.session)
        .unwrap()
        .unwrap()
        .read(&f.session)
        .is_ok());
    cache
        .wal
        .lock()
        .unwrap()
        .replace(&key, b"broken between requests")
        .unwrap();
    assert_eq!(
        cache.begin(&f.horizon).unwrap().state().sources[&0].end_sample,
        0
    );
}

#[test]
fn result_before_checkpoint_is_revalidated_and_reused_without_asr_or_duplicate_lines() {
    let f = Fixture::new([&[10, 20], &[10, 20]], true);
    let _lease = f.lease();
    let cache = Cache::open(&f.session, &f.identity).unwrap();
    let mut calls = 0;
    let first = cache
        .resolve(f.window(0, 0), |_| {
            calls += 1;
            Ok("  Первый текст  ".into())
        })
        .unwrap()
        .0;
    assert_eq!(
        cache.read_checkpoint(&f.horizon).unwrap().0.common_end,
        0,
        "uncommitted result is not progress"
    );
    let reopened = Cache::open(&f.session, &f.identity).unwrap();
    let (same, reused) = reopened
        .resolve(f.window(0, 0), |_| {
            panic!("accepted window must not repeat ASR")
        })
        .unwrap();
    assert!(reused);
    assert_eq!(same.text(), Some("Первый текст"));
    assert_eq!(calls, 1);
    let second = cache.resolve(f.window(0, 1), |_| Ok(".".into())).unwrap().0;
    assert_eq!(second.outcome(), &Output::Empty);
    let mut accepted = vec![first, second];
    for n in 0..2 {
        accepted.push(
            cache
                .resolve(f.window(1, n), |_| panic!("silence must not call ASR"))
                .unwrap()
                .0,
        );
    }
    let checkpoint = cache.checkpoint(&f.horizon, &accepted).unwrap();
    assert!(checkpoint.complete);
    assert_eq!(checkpoint.common_end, 20);
    cache.checkpoint(&f.horizon, &accepted).unwrap();
    let (state, records) = cache.read_checkpoint(&f.horizon).unwrap();
    assert_eq!(state, checkpoint);
    assert_eq!(
        records.iter().filter_map(|r| r.text()).collect::<Vec<_>>(),
        vec!["Первый текст"]
    );
    assert_eq!(
        records.len(),
        4,
        "same result was not duplicated at EOF retry"
    );
}

#[test]
fn corrupt_cache_rolls_back_only_its_source_prefix_and_repairs_only_missing_work() {
    let f = Fixture::new([&[10, 20, 30], &[10, 20, 30]], true);
    let _lease = f.lease();
    let cache = Cache::open(&f.session, &f.identity).unwrap();
    let mut all = Vec::new();
    for id in [0, 1] {
        for n in 0..3 {
            all.push(
                cache
                    .resolve(f.window(id, n), |_| Ok(format!("строка {n}")))
                    .unwrap()
                    .0,
            );
        }
    }
    cache.checkpoint(&f.horizon, &all).unwrap();
    let key = cache.key(&f.horizon.windows[&0][1]).unwrap();
    cache
        .wal
        .lock()
        .unwrap()
        .replace(&key, b"{partial result before sync")
        .unwrap();
    let (rolled, _) = cache.read_checkpoint(&f.horizon).unwrap();
    assert_eq!(rolled.sources[&0].end_sample, 10);
    assert_eq!(rolled.sources[&1].end_sample, 30);
    assert!(!rolled.complete);
    let mut calls = 0;
    let mut repaired = Vec::new();
    for id in [0, 1] {
        for n in 0..3 {
            repaired.push(
                cache
                    .resolve(f.window(id, n), |_| {
                        calls += 1;
                        Ok("восстановлено".into())
                    })
                    .unwrap()
                    .0,
            );
        }
    }
    assert_eq!(calls, 1);
    assert!(cache.checkpoint(&f.horizon, &repaired).unwrap().complete);
}

#[test]
fn failed_asr_and_refused_result_or_checkpoint_publish_never_advance_coverage() {
    let f = Fixture::new([&[10, 20], &[10, 20]], true);
    let _lease = f.lease();
    let cache = Cache::open(&f.session, &f.identity).unwrap();
    assert!(cache
        .resolve(f.window(0, 0), |_| anyhow::bail!("ASR timed out"))
        .is_err());
    let path = cache.root.join("windows").join(format!(
        "{}.json",
        cache.key(&f.horizon.windows[&0][0]).unwrap()
    ));
    fs::create_dir(&path).unwrap();
    assert!(cache
        .resolve(f.window(0, 0), |_| Ok("ответ".into()))
        .is_err());
    assert_eq!(
        cache.read_checkpoint(&f.horizon).unwrap().0.sources[&0].end_sample,
        0
    );
    fs::remove_dir(path).unwrap();
    let a = cache
        .resolve(f.window(0, 0), |_| Ok("ответ".into()))
        .unwrap()
        .0;
    cache.checkpoint(&f.horizon, &[a.clone()]).unwrap();
    let before = fs::read(cache.root.join("checkpoint.json")).unwrap();
    let path = cache.root.join("checkpoint.json");
    let backup = path.with_extension("saved");
    fs::rename(&path, &backup).unwrap();
    fs::create_dir(&path).unwrap();
    let b = cache
        .resolve(f.window(0, 1), |_| Ok("следующий".into()))
        .unwrap()
        .0;
    assert!(cache
        .checkpoint(&f.horizon, &[a.clone(), b.clone()])
        .is_err());
    assert_eq!(fs::read(&backup).unwrap(), before);
    fs::remove_dir(&path).unwrap();
    fs::rename(backup, &path).unwrap();
    let reopened = Cache::open(&f.session, &f.identity).unwrap();
    let (b, reused) = reopened
        .resolve(f.window(0, 1), |_| {
            panic!("synced result must survive refused checkpoint")
        })
        .unwrap();
    assert!(reused);
    assert_eq!(
        reopened.checkpoint(&f.horizon, &[a, b]).unwrap().sources[&0].end_sample,
        20
    );
}

#[test]
fn pcm_model_vocabulary_recipe_generation_and_range_are_all_dependencies() {
    let f = Fixture::new([&[10], &[10]], true);
    let _lease = f.lease();
    let cache = Cache::open(&f.session, &f.identity).unwrap();
    cache
        .resolve(f.window(0, 0), |_| Ok("исходник".into()))
        .unwrap();
    let d = &f.horizon.windows[&0][0];
    let mut changed = f.pcm[&0].clone();
    changed[1] ^= 1;
    assert!(VerifiedWindow::new(d, &changed).is_err());
    assert!(VerifiedWindow::new(d, &changed[..9]).is_err());
    for field in ["model", "vocab", "recipe", "language"] {
        let mut identity = f.identity.clone();
        if field == "recipe" {
            identity.recipe.push_str(" revised");
        } else if field == "language" {
            identity.language = "en".into();
        } else {
            let path = f.root.path().join(format!("changed-{field}"));
            fs::write(&path, b"new weights or vocabulary").unwrap();
            let changed = Identity::from_files(
                "fixture".into(),
                "ru".into(),
                "default recipe".into(),
                [(field.into(), path)].into(),
                || false,
            )
            .unwrap();
            identity
                .files
                .insert(field.into(), changed.files[field].clone());
        }
        let other = Cache::open(&f.session, &identity).unwrap();
        assert!(
            !other
                .resolve(f.window(0, 0), |_| Ok("новый результат".into()))
                .unwrap()
                .1
        );
    }
    for field in ["generation", "range", "planner"] {
        let mut other = d.clone();
        if field == "generation" {
            other.generation = uuid::Uuid::now_v7().to_string();
        } else if field == "range" {
            other.start_sample += 10;
            other.end_sample += 10;
        } else {
            other.recipe = hash(b"different planner recipe");
        }
        let verified = VerifiedWindow::new(&other, &f.pcm[&0]).unwrap();
        assert!(
            !cache
                .resolve(verified, |_| Ok("другая зависимость".into()))
                .unwrap()
                .1
        );
    }
}

#[test]
fn publication_order_and_audio_bound_do_not_insert_an_earlier_window_later() {
    let mut f = Fixture::new([&[8, 18], &[10, 20]], false);
    let _lease = f.lease();
    let cache = Cache::open(&f.session, &f.identity).unwrap();
    let a = cache
        .resolve(f.window(0, 0), |_| Ok("раньше".into()))
        .unwrap()
        .0;
    let b = cache
        .resolve(f.window(1, 0), |_| panic!("silence"))
        .unwrap()
        .0;
    let c = cache
        .resolve(f.window(1, 1), |_| panic!("silence"))
        .unwrap()
        .0;
    let s = cache.checkpoint(&f.horizon, &[a, b, c]).unwrap();
    assert_eq!(s.common_end, 8);
    let (_, before) = cache.read_checkpoint(&f.horizon).unwrap();
    assert_eq!(
        before
            .iter()
            .map(|r| r.descriptor().start_sample)
            .collect::<Vec<_>>(),
        vec![0, 0]
    );
    let a = cache
        .resolve(f.window(0, 1), |_| Ok("после".into()))
        .unwrap()
        .0;
    let s = cache.checkpoint(&f.horizon, &[a]).unwrap();
    // system [10,20) cannot yet play beyond captured mic end18; withhold from its start.
    assert_eq!(s.common_end, 18);
    assert_eq!(s.publication_end, 10);
    let (_, after) = cache.read_checkpoint(&f.horizon).unwrap();
    assert_eq!(
        after
            .iter()
            .map(|r| r.descriptor().start_sample)
            .collect::<Vec<_>>(),
        vec![0, 0, 8]
    );
    f.horizon.input.terminal = Some(capture::Terminal {
        at: "test".into(),
        reason: "test".into(),
        interrupted: false,
        ends: [(0, 18), (1, 20)].into(),
    });
    for source in f.horizon.plan.sources.values_mut() {
        source.eof = true;
    }
    f.horizon.plan.complete = true;
    f.horizon.plan.common_end = 20;
    let s = cache.checkpoint(&f.horizon, &[]).unwrap();
    assert!(s.complete);
    assert_eq!(s.publication_end, 20);
    let (_, final_rows) = cache.read_checkpoint(&f.horizon).unwrap();
    assert_eq!(
        final_rows
            .iter()
            .map(|r| r.descriptor().start_sample)
            .collect::<Vec<_>>(),
        vec![0, 0, 8, 10]
    );
}

#[test]
fn unpublished_temporary_result_is_ignored_and_foreign_or_stale_results_cannot_commit() {
    let f = Fixture::new([&[10, 20], &[10, 20]], false);
    let _lease = f.lease();
    let cache = Cache::open(&f.session, &f.identity).unwrap();
    let a = cache
        .resolve(f.window(0, 0), |_| Ok("первый".into()))
        .unwrap()
        .0;
    let path = cache.wal.lock().unwrap().location(&a.key);
    let pending = path.with_extension("pending");
    fs::rename(&path, &pending).unwrap();
    File::options()
        .read(true)
        .write(true)
        .open(&pending)
        .unwrap()
        .sync_all()
        .unwrap(); // complete temporary bytes are still not a published result
    assert_eq!(
        cache.read_checkpoint(&f.horizon).unwrap().0.sources[&0].end_sample,
        0
    );
    let mut calls = 0;
    let a = cache
        .resolve(f.window(0, 0), |_| {
            calls += 1;
            Ok("первый".into())
        })
        .unwrap()
        .0;
    assert_eq!(calls, 1);
    cache.checkpoint(&f.horizon, &[a.clone()]).unwrap();
    let original = fs::read(cache.root.join("checkpoint.json")).unwrap();
    let mut d = f.horizon.windows[&0][0].clone();
    d.generation = uuid::Uuid::now_v7().to_string();
    let foreign = cache
        .resolve(VerifiedWindow::new(&d, &f.pcm[&0][..10]).unwrap(), |_| {
            Ok("чужой".into())
        })
        .unwrap()
        .0;
    assert!(cache.checkpoint(&f.horizon, &[foreign]).is_err());
    assert_eq!(
        fs::read(cache.root.join("checkpoint.json")).unwrap(),
        original
    );
    let (mut newer, _) = read_envelope::<Checkpoint>(&cache.root.join("checkpoint.json")).unwrap();
    newer.capture_revision += 1;
    fs::write(
        cache.root.join("checkpoint.json"),
        envelope(&newer).unwrap(),
    )
    .unwrap();
    assert!(cache.checkpoint(&f.horizon, &[a]).is_err());
}

#[test]
fn corrupted_descriptor_and_invalid_empty_outcome_never_become_coverage() {
    let f = Fixture::new([&[10], &[10]], true);
    let _lease = f.lease();
    let cache = Cache::open(&f.session, &f.identity).unwrap();
    let a = cache
        .resolve(f.window(0, 0), |_| Ok("содержимое".into()))
        .unwrap()
        .0;
    let (mut bad, _) =
        decode_envelope::<Record>(&cache.wal.lock().unwrap().get(&a.key).unwrap().unwrap())
            .unwrap();
    bad.outcome = Output::Empty;
    bad.text = None;
    cache
        .wal
        .lock()
        .unwrap()
        .replace(&a.key, &envelope(&bad).unwrap())
        .unwrap();
    let mut calls = 0;
    let (fixed, reused) = cache
        .resolve(f.window(0, 0), |_| {
            calls += 1;
            Ok("исправлено".into())
        })
        .unwrap();
    assert!(!reused);
    assert_eq!(calls, 1);
    assert_eq!(fixed.outcome(), &Output::Text);
    let descriptor = &f.horizon.windows[&0][0];
    let path = f.session.join(descriptor.path());
    let before = fs::read(&path).unwrap();
    fs::write(&path, b"{truncated descriptor").unwrap();
    assert!(Horizon::read(&f.session).is_err());
    fs::write(&path, before).unwrap();
    assert!(Horizon::read(&f.session).is_ok());
    assert!(Identity::from_files(
        "fixture".into(),
        "ru".into(),
        "r".into(),
        [("model".into(), f.root.path().join("model.bin"))].into(),
        || true
    )
    .is_err());
}

#[test]
fn legacy_json_result_remains_reusable_without_asr_or_migration() {
    let f = Fixture::new([&[10], &[10]], true);
    let _lease = f.lease();
    let cache = Cache::open(&f.session, &f.identity).unwrap();
    let input = f.horizon.windows[&0][0].clone();
    let record = Record {
        version: 1,
        input: input.clone(),
        recognizer: cache.identity.clone(),
        outcome: Output::Text,
        raw_text: Some("старый результат".into()),
        text: Some("старый результат".into()),
    };
    let key = cache.key(&input).unwrap();
    let path = cache.root.join("windows").join(format!("{key}.json"));
    let bytes = envelope(&record).unwrap();
    fs::write(&path, &bytes).unwrap();
    let (accepted, reused) = cache
        .resolve(f.window(0, 0), |_| panic!("legacy hit must not call ASR"))
        .unwrap();
    assert!(reused);
    cache.checkpoint(&f.horizon, &[accepted]).unwrap();
    let reopened = Cache::open(&f.session, &f.identity).unwrap();
    assert_eq!(
        reopened.read_checkpoint(&f.horizon).unwrap().0.sources[&0].end_sample,
        10
    );
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert!(reopened.wal.lock().unwrap().get(&key).unwrap().is_none());
}

#[test]
fn malformed_descriptors_seals_and_forged_frontiers_cannot_advance_a_checkpoint() {
    let f = Fixture::new([&[10, 20], &[10, 20]], true);
    let _lease = f.lease();
    let cache = Cache::open(&f.session, &f.identity).unwrap();
    let a = cache
        .resolve(f.window(0, 0), |_| Ok("слово".into()))
        .unwrap()
        .0;
    cache.checkpoint(&f.horizon, &[a]).unwrap();
    let (mut state, _) = read_envelope::<Checkpoint>(&cache.root.join("checkpoint.json")).unwrap();
    state.common_end = u64::MAX;
    state.sources.get_mut(&0).unwrap().end_sample = u64::MAX;
    state.complete = true;
    fs::write(
        cache.root.join("checkpoint.json"),
        envelope(&state).unwrap(),
    )
    .unwrap();
    let (state, _) = cache.read_checkpoint(&f.horizon).unwrap();
    assert_eq!(state.sources[&0].end_sample, 10);
    assert_eq!(state.common_end, 0);
    assert!(!state.complete);
    let path = f.session.join(planning::FILE);
    let bytes = fs::read(&path).unwrap();
    for field in ["generation", "gap", "end", "eof"] {
        let mut plan: planning::Snapshot = serde_json::from_slice(&bytes).unwrap();
        match field {
            "generation" => plan.generation = uuid::Uuid::now_v7().to_string(),
            "gap" => plan.sources.get_mut(&0).unwrap().windows[0].start_sample = 1,
            "end" => plan.sources.get_mut(&0).unwrap().closed_end += 1,
            _ => {
                plan.sources.get_mut(&0).unwrap().sequence = 0;
            }
        }
        fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
        assert!(Horizon::read(&f.session).is_err());
    }
    fs::write(path, bytes).unwrap();
    let mut descriptor = f.horizon.windows[&0][0].clone();
    descriptor.recipe = "../../escape".into();
    assert!(VerifiedWindow::new(&descriptor, &f.pcm[&0][..10]).is_err());
    assert!(Identity::from_files(
        "e".into(),
        "ru".into(),
        "r".into(),
        [("model".into(), f.root.path().join("absent"))].into(),
        || false
    )
    .is_err());
}
