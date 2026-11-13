# Пользовательское хранилище моделей · 2026-11-13

Намеренное изменение по выбору владельца: `.localvox/models` в home на всех ОС.
Принятый main не переписывался; [delta](../specs/installation-models/spec.md)
добавляет SETUP-008 и расширяет SETUP-002. Переноса весов нет.

## Реализация

[Manifest](../../../../scripts/models.json) задаёт placement/env четырёх моделей.
[Core resolver](../../../../crates/localvox-light-core/src/model_paths.rs) подключён
к Vosk, GigaAM CLI/runtime, side models, doctor и setup. Explicit paths имеют
приоритет; portable/CWD сохраняются. Setup предлагает home/.localvox как root.
CLI `--model-paths-json` schema 1 показывает paths/source/payload presence без
запуска inference, устройств, сети и записи. CLI override различается через Clap
value_source, env/.env читает общий resolver.

[PS](../../../../scripts/model-paths.ps1) и [Bash](../../../../scripts/model-paths.sh)
используют native JSON в bundle. Standalone без .env читает manifest/env;
с .env и без binary требует explicit Root вместо shell eval/частичного парсера.
Check/repeat не создают store/не переписывают готовые файлы. Только новая стандартная
папка Windows получает Hidden. Перед удалением ZIP destination проверяются marker
новой распаковки, путь компонента и отсутствие symlink/reparse points. Broken
explicit path требует настройки/repair, не подменяется другим скачиванием.
Installer не закрепляет модели за install dir; готовая .env сохраняется.
Ollama store, архив и inference не менялись.

## Проверки Windows

Rust 1.99.1, x86_64-pc-windows-msvc, jobs=2, fixtures на D:. Личный демон,
конфиг, архив и веса не менялись.

- `cargo test -p localvox-light-core -p localvox-light --lib --bins --tests`:
  core **447 passed / 9 ignored**, product lib **16 passed / 11 ignored**, main
  **2 passed**, setup **10 passed**, copied-process model paths **1 passed**;
  launcher **1 ignored**, fbank без выбранных fixtures — 0.
  [Unit](../../../../crates/localvox-light-core/src/model_paths/tests.rs) проверяет
  home/portable/cwd/explicit, отсутствие home, Unicode, ASR siblings и manifest.
  [Процесс](../../../../crates/localvox-light/tests/model_paths.rs) — настоящий
  copied binary, .env/CLI priority и read-only поведение.
- `cargo check -p localvox-light-asr --bins`: **passed**.
- [PS/Bash fixtures](../../../../scripts/tests/test_model_storage_delivery.py):
  **6 passed**: настоящие tiny localhost downloads, hash/ZIP, Check, повтор без
  HTTP/перезаписи, Windows Hidden, Root/portable/external, broken explicit, .env,
  native resolver. Shell AppData cache исключён из product snapshot; уменьшены
  jq subprocesses, исправлена нормализация MSYS aliases /d/... и /tmp для Hidden.
- Delivery без архива: **19 passed / 1 skipped**; CI contracts **4 passed / 1 skipped**.
- Полная Windows release сборка (daemon/worker/TUI/desktop), package smoke и
  `check-release-install.py` на freshly packaged archive: **passed**. Doctor
  обнаружил deliberately missing ASR, setup-check/cancel/no-op/reconcile и fetch
  Check не изменили конфигурацию/каталоги. Затем весь `test_*delivery.py` с
  `LOCALVOX_DELIVERY_ARCHIVE`: **36 passed, 0 skipped** (187.238 s).
  После уточнения no-write для готовых explicit моделей комплект перепакован;
  все **6** model-storage fixtures повторно **passed** на настоящем release archive
  (54.578 s), включая отсутствие license sidecar у external модели. Режимы
  default/Root по-прежнему создают license из manifest при новой загрузке.
- Strict main **15 passed**, **40** workflow changes **passed**, check-openspec
  **passed** (15 capabilities / 88 main requirements / 188 scenarios).
  Изменённые Rust файлы rustfmt, Bash syntax, actionlint **passed**.
  `cargo fmt --all --check` обнаруживает прежнее форматирование вне этой работы;
  посторонние модули не форматировались.

Логи: `target/tmp/user-models-{rust,release,downloads,downloads-final,delivery,ci-delivery,spec-checks,package,install,all-delivery}.log`.

## Границы

Native Mac/Linux, аппаратный звук/разрешения, настоящий multi-GB download и новый
hosted run — **not-run**. CI проверяет native resolver после package relocation.
Payload presence не доказывает inference/качество или hash готовых файлов.
