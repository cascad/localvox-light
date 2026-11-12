use anyhow::{ensure, Result};
use std::path::Path;
use winreg::{
    enums::*,
    types::{FromRegValue, ToRegValue},
    RegKey, RegValue,
};

fn contains(value: &str, directory: &Path) -> bool {
    std::env::split_paths(value).any(|p| super::same(&p, directory))
}

fn optional(key: &RegKey) -> Result<Option<RegValue>> {
    match key.get_raw_value("Path") {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn text(value: &Option<RegValue>) -> Result<String> {
    value
        .as_ref()
        .map(String::from_reg_value)
        .transpose()
        .map(|s| s.unwrap_or_default())
        .map_err(Into::into)
}

fn expanded(value: &Option<RegValue>) -> Result<String> {
    let raw = text(value)?;
    if !value.as_ref().is_some_and(|v| v.vtype == REG_EXPAND_SZ) {
        return Ok(raw);
    }
    use windows::{core::PCWSTR, Win32::System::Environment::ExpandEnvironmentStringsW};
    let input: Vec<u16> = raw.encode_utf16().chain(std::iter::once(0)).collect();
    let needed = unsafe { ExpandEnvironmentStringsW(PCWSTR(input.as_ptr()), None) };
    ensure!(
        needed > 0,
        "Не удалось раскрыть переменные пользовательского PATH"
    );
    let mut output = vec![0u16; needed as usize];
    let written = unsafe { ExpandEnvironmentStringsW(PCWSTR(input.as_ptr()), Some(&mut output)) };
    ensure!(
        written > 0 && written <= needed,
        "Окружение изменилось при проверке PATH; повторите установку"
    );
    Ok(String::from_utf16(&output[..written as usize - 1])?)
}

fn system_path() -> Result<String> {
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment")?;
    expanded(&optional(&key)?)
}

pub(super) fn registered(directory: &Path) -> Result<bool> {
    let user = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Environment")?;
    Ok(contains(&expanded(&optional(&user)?)?, directory) || contains(&system_path()?, directory))
}

fn update(key: &RegKey, directory: &Path) -> Result<bool> {
    let before = optional(key)?;
    let old = text(&before)?;
    if contains(&expanded(&before)?, directory) {
        return Ok(false);
    }
    let path = directory
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("PATH требует Unicode путь"))?;
    ensure!(
        !path.contains([';', '\r', '\n']),
        "Каталог нельзя добавить в PATH: {path}"
    );
    let next = format!(
        "{old}{}{path}",
        if old.is_empty() || old.ends_with(';') {
            ""
        } else {
            ";"
        }
    );
    ensure!(
        next.encode_utf16().count() < 32767,
        "Пользовательский PATH слишком длинный"
    );
    let mut value = next.to_reg_value();
    value.vtype = before
        .as_ref()
        .map(|v| v.vtype.clone())
        .unwrap_or(REG_EXPAND_SZ);
    let fresh = optional(key)?;
    ensure!(
        fresh.as_ref().map(|v| (&v.bytes, &v.vtype))
            == before.as_ref().map(|v| (&v.bytes, &v.vtype)),
        "Пользовательский PATH изменился; повторите установку"
    );
    key.set_raw_value("Path", &value)?;
    Ok(true)
}

pub(super) fn install(directory: &Path) -> Result<bool> {
    if contains(&system_path()?, directory) {
        return Ok(false);
    }
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey("Environment")?;
    let changed = update(&key, directory)?;
    if changed {
        use windows::Win32::{
            Foundation::{LPARAM, WPARAM},
            UI::WindowsAndMessaging::*,
        };
        let environment: Vec<u16> = "Environment\0".encode_utf16().collect();
        unsafe {
            let _ = SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(environment.as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                2000,
                None,
            );
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolated_registry_value_preserves_type_and_other_entries_and_is_idempotent() {
        // Never open the real Environment key for writes in this test.
        let name = format!(r"Software\LocalVox\Tests\{}", std::process::id());
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu.create_subkey(&name).unwrap();
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.0);
            }
        }
        let _cleanup = Cleanup(name);
        let mut old = r"%USERPROFILE%\bin;C:\existing".to_reg_value();
        old.vtype = REG_EXPAND_SZ;
        key.set_raw_value("Path", &old).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("LocalVox с пробелом");
        std::fs::create_dir(&directory).unwrap();
        assert!(update(&key, &directory).unwrap());
        let before = optional(&key).unwrap().unwrap();
        assert_eq!(before.vtype, REG_EXPAND_SZ);
        assert!(String::from_reg_value(&before)
            .unwrap()
            .starts_with(r"%USERPROFILE%\bin;C:\existing;"));
        assert!(!update(&key, &directory).unwrap());
        assert_eq!(optional(&key).unwrap().unwrap().bytes, before.bytes);
        let relative = directory.strip_prefix(std::env::temp_dir()).unwrap();
        let mut symbolic = format!(r"%TEMP%\{}", relative.display()).to_reg_value();
        symbolic.vtype = REG_EXPAND_SZ;
        key.set_raw_value("Path", &symbolic).unwrap();
        assert!(
            !update(&key, &directory).unwrap(),
            "expanded PATH must not gain a duplicate"
        );
        assert_eq!(optional(&key).unwrap().unwrap().bytes, symbolic.bytes);
    }
}
