use super::*;
#[test]
fn append_preserves_other_bytes_and_repeat_preserves_mtime() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("profile");
    let original = b"# owner config\r\nexport KEEP=1";
    fs::write(&path, original).unwrap();
    let body = block(Path::new("/opt/LocalVox's journal"), Shell::Posix).unwrap();
    let edit = Edit::plan(path.clone(), &body, false).unwrap();
    assert!(edit.apply().unwrap());
    assert!(fs::read(&path).unwrap().starts_with(original));
    let bytes = fs::read(&path).unwrap();
    let time = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(!Edit::plan(path.clone(), &body, false)
        .unwrap()
        .apply()
        .unwrap());
    assert_eq!(bytes, fs::read(&path).unwrap());
    assert_eq!(time, fs::metadata(&path).unwrap().modified().unwrap());
}

#[test]
fn concurrent_profile_edit_and_foreign_managed_block_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("profile");
    fs::write(&path, "owner\n").unwrap();
    let body = block(Path::new("/opt/LocalVox"), Shell::Posix).unwrap();
    let edit = Edit::plan(path.clone(), &body, false).unwrap();
    fs::write(&path, "new owner data\n").unwrap();
    assert!(edit.apply().is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "new owner data\n");
    fs::write(&path, format!("{BEGIN}\nforeign\n{END}\n")).unwrap();
    assert!(Edit::plan(path.clone(), &body, false).is_err());
    assert!(Edit::plan(path, &body, true).is_err());
}

#[test]
fn planning_never_creates_profiles_and_respects_shell_locations() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let bash = targets(home, "/bin/bash", None, None).unwrap();
    assert_eq!(bash[1].0, home.join(".profile"));
    fs::write(home.join(".bash_login"), "# existing").unwrap();
    assert_eq!(
        targets(home, "bash", None, None).unwrap()[1].0,
        home.join(".bash_login")
    );
    let custom = home.join("custom");
    assert_eq!(
        targets(home, "zsh", Some(custom.clone()), None).unwrap()[0].0,
        custom.join(".zshrc")
    );
    assert_eq!(
        targets(home, "fish", None, Some(custom.clone())).unwrap()[0].0,
        custom.join("fish/conf.d/localvox.fish")
    );
    assert!(targets(home, "unknown-shell", None, None).is_err());
    assert!(!custom.exists());
    let body = block(Path::new("/opt/LocalVox"), Shell::Fish).unwrap();
    let edit = Edit::plan(custom.join("profile"), &body, true).unwrap();
    assert!(edit.changed());
    assert!(!custom.exists());
    assert!(block(Path::new("/opt/bad:path"), Shell::Posix).is_err());
}

#[test]
fn posix_block_quotes_values_and_does_not_duplicate_or_execute_them() {
    let root = tempfile::tempdir().unwrap();
    let directory = "/opt/LocalVox's $(touch injected) журнал";
    let body = block(Path::new(directory), Shell::Posix).unwrap();
    let bash = if cfg!(windows) {
        r"C:\Program Files\Git\bin\bash.exe"
    } else {
        "bash"
    };
    let output = std::process::Command::new(bash)
        .args([
            "--noprofile",
            "--norc",
            "-c",
            &format!("PATH=/usr/bin:/bin\n{body}{body}printf '%s' \"$PATH\""),
        ])
        .current_dir(root.path())
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("/usr/bin:/bin:{directory}")
    );
    assert!(!root.path().join("injected").exists());
}

#[cfg(unix)]
#[test]
fn linked_profile_target_and_permissions_are_preserved() {
    use std::os::unix::{fs::symlink, fs::PermissionsExt};
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("dotfiles");
    fs::write(&target, "# owner\n").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    let link = root.path().join("profile");
    symlink(&target, &link).unwrap();
    let body = block(Path::new("/opt/LocalVox"), Shell::Posix).unwrap();
    Edit::plan(link.clone(), &body, false)
        .unwrap()
        .apply()
        .unwrap();
    assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
    assert_eq!(
        fs::metadata(target).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[cfg(unix)]
#[test]
fn zsh_and_fish_execute_their_generated_blocks_once() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("LocalVox's журнал");
    fs::create_dir(&directory).unwrap();
    for (program, shell) in [("zsh", Shell::Posix), ("fish", Shell::Fish)] {
        // Resolve with the caller's PATH before the child loses Homebrew's bin.
        let executable = which::which(program)
            .unwrap_or_else(|error| panic!("Required test shell {program}: {error}"));
        let body = block(&directory, shell).unwrap();
        let script = format!("{body}{body}printf '%s' \"$PATH\"");
        let output = std::process::Command::new(executable)
            .arg(if program == "fish" {
                "--no-config"
            } else {
                "-f"
            })
            .args(["-c", &script])
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", root.path())
            .env_remove("ZDOTDIR")
            .env_remove("XDG_CONFIG_HOME")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{program}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        // Fish joins PATH as a colon-delimited value inside double quotes.
        assert_eq!(
            String::from_utf8_lossy(&output.stdout)
                .matches(directory.to_str().unwrap())
                .count(),
            1
        );
    }
}

#[cfg(windows)]
#[test]
fn denied_atomic_replacement_keeps_original_profile() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("profile");
    fs::write(&path, "owner data\n").unwrap();
    let edit = Edit::plan(
        path.clone(),
        &block(Path::new("/opt/LocalVox"), Shell::Posix).unwrap(),
        false,
    )
    .unwrap();
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    assert!(edit.apply().is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "owner data\n");
    drop(handle);
    assert!(edit.apply().unwrap());
}
