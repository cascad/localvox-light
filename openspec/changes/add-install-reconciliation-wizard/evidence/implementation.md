# Implementation evidence · 2026-11-12

SETUP-005/006 остаются delta этого change. SETUP-001/002/003 и INV-SETUP-001
не переопределены; main specs не sync/archive. Реализация отделена от аппаратной
и first-run приёмки.

## Code-reviewed

- `crates/localvox-light/src/setup/{probe,plan,prompt,execute}.rs`: read-only probes,
  typed plan, terminal choices, explicit confirmation and separate executor.
- `crates/localvox-light-core/src/doctor.rs`: stable finding IDs / Serialize;
  runtime path rules сохранены, selected tag проверяется через `/api/tags`.
- CLI one-shot modes до model/device/engine startup: `--setup`, `--setup-check`,
  `--doctor --doctor-json` (schema 1). JSON содержит пути, но не токены и не список
  всех пользовательских моделей. CLI/API/selected tag — отдельные findings.
- Keep не закрепляет presets overrides. Изменённая `.env` публикуется атомарно
  через существующий artifact writer с проверкой concurrent edit до/после работы;
  no-op не пишет даже файл блокировки. Применение получает lock архива, которым
  владеет engine; демон не останавливается автоматически.
- ASR download использует manifest loaders по выбранному component/root, после
  него проверяет комплект снова. Complete component вообще не вызывает loader.
  Existing paths используются без копирования. Partial ZIP repair виден в плане.
- Ollama tags — bounded read-only HTTP. Pull проверяет streaming errors и terminal
  success, затем rechecks tag. Перед pull/import повторно проверяется наличие;
  существующий tag не заменяется. GGUF header/local endpoint/new name проверяются
  до плана; импорт — стандартный Ollama Modelfile/CLI, без shell interpolation.
- Windows/macOS handoff открывает официальный installer/application image, вне
  kill-on-close worker jobs. Windows проверяет Authenticode, Mac проверяет DMG.
  Рабочая Ollama не переустанавливается и не перезапускается. Проверенный installer
  cache переиспользуется; storage существующего сервера не меняется.
- Installer Check работает до scratch allocation и без скачивания. Reconcile
  сверяет установленный bundle manifest/hashes и сохраняет текущую версию.
  Wizard вызывается до legacy model downloader / создания `.env`. Старые
  Archive/RequiredOnly/SkipModels сценарии сохранены.
- CI: setup integration/doctor добавлены в interface workflow, новые helpers и
  manifest/downloaders в package и build trigger paths. README и US-046/059/060
  описывают доступные действия.

## Test-run

Все temporary files / build outputs на D:, TEMP/TMP=`target/tmp`, cargo jobs=2.
Рабочий архив, daemon, Ollama и пользовательские модели не менялись.

- `cargo check -p localvox-light --offline --jobs 2` — PASS.
- `cargo test --release --locked --offline --jobs 2 -p localvox-light --test setup
  --lib --bin localvox-light` — 10 setup integration, 16 lib, 1 main PASS;
  11 прежних subprocess/helper lib tests ignored по их штатной маркировке.
- `cargo test --release --locked --offline --jobs 2 -p localvox-light-core --lib
  doctor::` — 6 PASS.
- Setup integration: настоящий CLI JSON с внешними моделями; отмена/EOF/no-op
  без файлов; invalid source/GGUF/credentials/tag; настоящий HTTP server считает
  GET/pull (1 pull за 2 attempts); API ready / model missing различаются;
  streaming error/truncation; config/mtime/comment preservation; concurrent edit;
  engine lock; настоящий PowerShell fetch subprocess запускается один раз,
  повторный apply его пропускает.
- `python scripts/package-release.py --target x86_64-pc-windows-msvc --source
  target/modular-build/release --output target/setup-delivery-check` — полный
  native Windows bundle, startup smoke PASS. Desktop не пересобирался/не включён
  в этот локальный runtime package; исходная CI сборка desktop сохранена.
- `LOCALVOX_DELIVERY_ARCHIVE=<new package> python -m unittest discover -s
  scripts/tests -p test_release_delivery.py` — 16 PASS, без skips: настоящая Windows
  установка/обновление/повторный reconcile/check, byte+mtime preservation;
  readonly check до первой установки; Mac arm64/Intel shell fixtures,
  missing archive на no-op не открывается, corruption check, wizard до downloads.
- Bash syntax, PowerShell parser, actionlint обоих затронутых workflows — PASS.
- Strict main specs: 15 passed / 0 failed. Strict change valid;
  check-openspec PASS. Scoped rustfmt / diff check PASS.

## Not-run / limits

- Настоящая новая установка Ollama на Windows/macOS, OS permissions, настоящий
  большой pull, первый GGUF create и поддержка конкретной архитектуры GGUF —
  **not-run**. Это 4.2 владельца, не закрыто unit/fixture тестами. Установка на
  самом Mac/hosted GitHub jobs тоже не запускались в этой работе.
- Automatic review отклонил добавление теста, имитирующего подпись installer,
  с `blocked by policy` без подробного объяснения. Тест не добавлен и не запускался;
  signature/official installer acceptance остаётся ручной, обходов нет.
- Doctor проверяет структуру/наличие файлов, не inference или полную пригодность
  весов. GLiNER discovery пока проверяет tokenizer/config как прежний doctor;
  файл/качество весов подтверждаются настоящей загрузкой отдельно. Warning для
  custom OpenAI-compatible server явно говорит, что `/api/tags` неприменим.
- GGUF import требует локальную Ollama CLI, обычный путь без кавычек/переводов строк,
  отдельное новое имя и дополнительное место в хранилище Ollama. Sharded import
  в этом single-file мастере не заявлен. Пользовательские модели не переключались.
