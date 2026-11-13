# Native CI окружение · 2026-11-13

## Подтверждённые отказы

На HEAD 2346911: [Build binaries 43291458096](https://github.com/cascad/localvox-light/actions/runs/43291458096)
и [Interface 43291458202](https://github.com/cascad/localvox-light/actions/runs/43291458202)
failed. Linux x64/ARM passed; Mac Intel/ARM скомпилировались, затем упали в shell
fixture. Windows остановился при build ort-sys, interface — при onnx cook tests.

- Windows jobs 131438483704 / 131438479078: Rust 1.100.0, ort-sys rc.12,
  dynamic_link.rs:42, OS error 3. OUT_DIR новой структуры имеет дополнительный
  уровень, ancestors(3) указывает на build. Лог rerun-if-changed показывает DLL
  в release/build, не рядом с executable. Это согласуется с
  [upstream исправлением](https://github.com/pykeio/ort/commit/eed56816088613b24f8d1a5bf9ae0fd43d8d6a70)
  и новым vars::target_dir. Локальный registry не патчился.
- Mac jobs 131438483144 / 131438483782: test zsh_and_fish_execute_their_generated_blocks_once,
  profiles/tests.rs:143, ENOENT. Test заменял PATH на /usr/bin:/bin до разрешения
  executable; Homebrew Fish вне этого PATH. Новая which lookup выполняется раньше,
  абсолютный путь передаётся Command; child HOME/PATH/profile isolation сохранена.
- [Предыдущий зелёный прогон 43262427328](https://github.com/cascad/localvox-light/actions/runs/43262427328)
  на 8900c58: все пять native jobs success. Проверены логи всех пяти jobs:
  rustc 1.99.1. Эти исторические результаты не выдаются за run нового commit.

## Исправление

rust-toolchain.toml закрепляет 1.99.1/minimal/rustfmt. Оба workflow парсят общий
TOML как UTF-8 и передают channel в action через step output. Test исполняет
сам parser из workflow на текущем и изменённом channel, с Unicode comment;
проверяет wiring и отсутствие moving stable. Изменение pin запускает Build workflow.
Dev guide описывает toolchain и установку без двойной загрузки stable + pinned.

which 8.0.6 добавлен только Unix dev dependency; Cargo.lock изменился только этим
пакетом. ONNX Runtime 1.24.2 / ort rc.12, модели и runtime функции не менялись.
Upstream rc.15 использует SDK 1.31, поэтому его миграция не включена в эту работу.

## Выполненные локальные проверки

RUSTUP_HOME=target/tools/rustup-native-ci, CARGO_TARGET_DIR=target/native-ci-build,
TEMP/TMP=target/tmp, jobs=2. Toolchain/stdlib скачаны на D; пользовательский
rustup default и пользовательские archive/.env/PATH не изменялись.

- Rust 1.99.1: release daemon/ASR/worker/TUI/vox build passed (4m30s),
  desktop build passed (2m53s). DLL DirectML находится в release рядом с exe.
- TUI: 212 lib passed / 3 fixture-only ignored, 2 IO, 8 PTY, 4 launcher passed.
  Media case явно ignored. Snapshot baselines сохранены.
- Полный Windows zip с desktop собран, package startup probes passed.
  30 delivery tests passed без skips (104.282 s): actual installed archive,
  doctor, setup cancel/no-op, reconcile, модели отсутствуют намеренно.
  Установки используют NoPathUpdate; registry/profile checks изолированы.

Дополнительно:

- 9 core cook tests с features=onnx passed на 1.99.1. Первый explicit-target
  запуск не дошёл до harness из-за runtime DLL search path; остановлен только
  принадлежащий стенду PID после проверки path/parent. Отдельный bounded --list
  подтвердил 0xC0000135 без native PATH и успех с путями Vosk/profile. Повтор
  с этими путями passed, содержимое тестов не менялось.
- Полный frontend Linux cross-check упёрся в отсутствие x86_64-linux-gnu-gcc
  на Windows. Не объявляется pass. Вместо установки C toolchain отдельно
  скомпилированы реальные command_path/profiles/tests для Linux: byte-identical
  copies модулей в isolated crate, which 8.0.6, cargo check --tests passed.
  Это проверка изменённого Unix кода, не выполнение Zsh/Fish и не полный TUI build.

Логи: target/tmp/native-ci-{toolchain,terminal,product-build,desktop-build,package,delivery}.log,
native-ci-core-cook-runtime-path.log, native-ci-unix-module-verified.log.
Исходные неуспешные попытки сохранены отдельно в native-ci-core-cook.log и
native-ci-unix-check.log. Final documentation checks: strict main/change specs,
check-openspec, API/scoped formatting, actionlint и diff.

Native Mac/Fish/Zsh execution, Linux native execution и новый hosted run — not-run
до push владельца. Cross-check compilation не считается native приёмкой.
