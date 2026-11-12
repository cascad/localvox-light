# Проверка сохранения настроек · 2026-11-12

## Scope / code-reviewed

Ограниченный внутренний срез CFG-001/002/003 и SETUP-006 delta. Main specs,
HTTP/CLI/DTO, model recipes и restart/live policies не меняются. `skip_specs: true`.

- `core/env_file::Snapshot` связывает destination и наблюдаемое состояние файла;
  missing/empty различаются. Verify не пишет файлы. Save проверяет снимок перед
  публикацией, не печатает content/credentials и использует прежний atomic writer.
- API сохраняет через прежние `save`/`save_to`. Read-modify-write сериализован
  внутри процесса: independent HTTP edits не теряют чужие keys. Не заявлена
  общая cross-process CAS/transaction с произвольными внешними редакторами.
- Setup больше не дублирует read/check/write; `Snapshot` получается до выбора
  и подтверждения, проверяется до действий и сохраняется после них. Workspace
  ownership, completed models и отсутствие записи на отмене/no-op сохранены.
- Семантический no-op сохраняет исходные bytes/quotes/comments/line endings/mtime;
  повторные edits одного ключа применяются в прежнем порядке. Renderer `apply`
  не менялся. Новый no-op test выявил старый дефект quoted inline comment;
  `unquote` теперь отличает comment после closing quote от hash внутри value.
- Existing symlink сохраняется через destination; dangling link не заменяется
  новым конфигом. Unix branch имеет test, но на Windows он не запускался.

## Test-run

Все файлы fixtures в TEMP/TMP=`target/tmp` на D:, cargo jobs=2, offline/locked.
Рабочие `.env`, archive/daemon, Ollama/модели не использовались для записи.

| Command / boundary | Result |
|---|---|
| `cargo test --release --locked --offline --jobs 2 -p localvox-light-core --lib env_file::` | 20 passed / 0 failed |
| `cargo test --release --locked --offline --jobs 2 -p localvox-light-api --lib http::tests::settings::` | 1 parent test passed, 1 helper ignored in discovery; parent реально запускает copied executable/private cwd HTTP fixture |
| `cargo test --release --locked --offline --jobs 2 -p localvox-light --test setup --lib --bin localvox-light` | 16 lib + 1 main + 10 setup passed; 11 прежних helper/device lib tests ignored |

Core: реальные независимые concurrent edits, stale snapshot, missing→empty,
неизменённые CRLF/quotes/inline comment/mtime, quoted literal hashes и Windows
paths; реальный read-denied handle и replace-denied handle. При denied replace
исходный файл цел, temporary file убран; после освобождения handle retry успешен.

HTTP: существующие masking/secrets/presets/reset/need_restart/live flags и checks
ошибок сохранены. Добавлены no-op byte/mtime и настоящий rename denial; POST
возвращает ошибку, байты и process model остаются прежними (child result проверен).

Setup: отмена/EOF/no-op, pull/error/recheck и прежние ownership checks. Настоящий
fetch subprocess теперь также изменяет private `.env` во время работы: модели
готовы, но stale save отклоняется. Повтор с новым snapshot пропускает fetch,
сохраняет внешний key/comment и добавляет только выбранную настройку.

## Limits

Power-loss, disk-full, Unix/macOS symlink/permissions и аппаратный Mac прогон
**not-run**. Atomic primitive уже существует; bounded Windows retry не расширялся.
Из этого среза не следует общая транзакционность внешних editors или нескольких
процессов. Порядок apply_live после HTTP save сохранён, не расширен до глобальной
конфигурационной транзакции.

UI/TUI source не менялся: US-036/037 и US-059/060 имеют прежние действия/результат,
поэтому user-guide не переписывался. Остальные крупные HTTP/queue/LLM модули и BL-006
вне среза. Новая установка Ollama/большие downloads/Mac acceptance прежнего setup
change остаются not-run. Main specs не sync/archive.

## Final checks

Release build `cargo build --release --locked --offline --jobs 2 -p localvox-light`
прошёл. Fresh Windows runtime package в `target/setup-delivery-check` собран и
прошёл native startup smoke; UI/TUI/worker source не менялись.
Strict main specs: 15 passed / 0 failed; strict change valid; check-openspec PASS
(15 capabilities / 88 requirements / 188 scenarios / 88 coverage rows / 1931 links).
Scoped rustfmt и diff check PASS. Все 3 задачи среза завершены; push не выполняется.
