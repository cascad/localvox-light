use super::*;

fn context(root: &Path) -> Context {
    Context {
        home: Some(root.join("Юзер с пробелами")),
        exe: Some(root.join("install")),
        cwd: Some(root.join("workspace")),
        overrides: BTreeMap::new(),
    }
}

#[test]
fn fresh_defaults_are_shared_lowercase_and_readonly() {
    let root = tempfile::tempdir().unwrap();
    let ctx = context(root.path());
    let base = ctx.home.as_ref().unwrap().join(".localvox");
    assert_eq!(ctx.user_root(), Some(base.clone()));
    for found in ctx.locations() {
        assert_eq!(found.path, Some(base.join(&component(&found.id).directory)));
        assert_eq!(found.source, Source::User);
        assert!(!found.ready);
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn explicit_invalid_and_quoted_relative_paths_never_fall_through() {
    let root = tempfile::tempdir().unwrap();
    let mut ctx = context(root.path());
    let c = component("gigaam");
    let portable = ctx.exe.as_ref().unwrap().join(&c.directory);
    std::fs::create_dir_all(&portable).unwrap();
    ctx.overrides
        .insert(c.env.clone(), "  'другой диск/модель'  ".into());
    let found = ctx.resolve(c, None);
    assert_eq!(
        found.path,
        Some(ctx.cwd.as_ref().unwrap().join("другой диск/модель"))
    );
    assert_eq!(found.source, Source::Explicit);
    assert!(!found.ready);
    ctx.overrides.insert(c.env.clone(), String::new());
    assert_eq!(ctx.resolve(c, None).path, Some(PathBuf::new()));
    assert_eq!(ctx.resolve(c, None).source, Source::Explicit);
}

#[test]
fn portable_then_cwd_remain_usable_without_home() {
    let root = tempfile::tempdir().unwrap();
    let mut ctx = context(root.path());
    ctx.home = None;
    let c = component("vosk");
    assert!(ctx.resolve(c, None).path.is_none());
    ctx.home = Some(PathBuf::from("relative-home"));
    assert!(ctx.resolve(c, None).path.is_none());
    let cwd = ctx.cwd.as_ref().unwrap().join(&c.directory);
    std::fs::create_dir_all(&cwd).unwrap();
    assert_eq!(ctx.resolve(c, None).path, Some(cwd));
    assert_eq!(ctx.resolve(c, None).source, Source::Cwd);
    let portable = ctx.exe.as_ref().unwrap().join(&c.directory);
    std::fs::create_dir_all(&portable).unwrap();
    assert_eq!(ctx.resolve(c, None).path, Some(portable));
    assert_eq!(ctx.resolve(c, None).source, Source::Portable);
}

#[test]
fn side_models_follow_supplied_then_configured_asr_before_user_store() {
    let root = tempfile::tempdir().unwrap();
    let mut ctx = context(root.path());
    let supplied = root.path().join("supplied/asr");
    let configured = root.path().join("configured/asr");
    let name = component("ner").directory.file_name().unwrap();
    let supplied_ner = supplied.parent().unwrap().join(name);
    let configured_ner = configured.parent().unwrap().join(name);
    std::fs::create_dir_all(&configured_ner).unwrap();
    ctx.overrides.insert(
        component("gigaam").env.clone(),
        configured.display().to_string(),
    );
    assert_eq!(
        ctx.resolve(component("ner"), Some(&supplied)).path,
        Some(configured_ner)
    );
    std::fs::create_dir_all(&supplied_ner).unwrap();
    assert_eq!(
        ctx.resolve(component("ner"), Some(&supplied)).path,
        Some(supplied_ner)
    );
    ctx.overrides.insert(
        component("ner").env.clone(),
        root.path().join("missing-ner").display().to_string(),
    );
    assert_eq!(
        ctx.resolve(component("ner"), Some(&supplied)).source,
        Source::Explicit
    );
    assert!(!ctx.resolve(component("ner"), Some(&supplied)).ready);
}

#[test]
fn manifest_presence_matches_payload_files_and_zip_marker() {
    let root = tempfile::tempdir().unwrap();
    let c = component("gigaam");
    for file in [
        "v3_e2e_ctc.int8.onnx",
        "v3_e2e_ctc.yaml",
        "v3_e2e_ctc_vocab.txt",
    ] {
        assert!(!c.ready(root.path()));
        std::fs::write(root.path().join(file), b"synthetic presence only").unwrap();
    }
    assert!(c.ready(root.path()));
    let vosk = component("vosk");
    assert!(!vosk.ready(root.path()));
    std::fs::create_dir(root.path().join("am")).unwrap();
    std::fs::write(root.path().join("am/final.mdl"), b"synthetic presence only").unwrap();
    assert!(vosk.ready(root.path()));
}

#[test]
fn manifest_placement_is_complete_unique_and_cannot_escape_a_component() {
    let mut ids = std::collections::BTreeSet::new();
    let mut envs = std::collections::BTreeSet::new();
    for c in components() {
        assert!(ids.insert(&c.id));
        assert!(envs.insert(&c.env));
        assert!(c.directory.starts_with("models"));
        assert!(c
            .directory
            .components()
            .all(|p| matches!(p, std::path::Component::Normal(_))));
        if let Some(parent) = &c.sibling_of {
            assert!(component(parent).sibling_of.is_none(), "model parent cycle");
        }
        for item in &c.items {
            let path = item.dest.as_ref().or(item.unpack_to.as_ref()).unwrap();
            assert!(path.starts_with(&c.directory));
            assert!(path
                .components()
                .all(|p| matches!(p, std::path::Component::Normal(_))));
        }
    }
    assert_eq!(ids.len(), 4);
}
