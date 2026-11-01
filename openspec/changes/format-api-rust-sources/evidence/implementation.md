# Форматирование API · 2026-11-01

Продолжение очистки проекта после fix-workspace-status-polling. Исходное рабочее
дерево было чистым. Поведение не менялось; `skip_specs: true`, новых требований
и delta нет. Условный design не создавался: архитектурных решений здесь нет,
CLI instructions apply вернул ready с имеющимися proposal/tasks.

## Исходный отказ и изменения

`cargo fmt -p localvox-light-api -- --check` до правки: FAIL, замечания в шести
файлах. Использован rustfmt 1.9.0-stable (31fca3adb2, 2026-06-26).
После `cargo fmt -p localvox-light-api` тот же check проходит.

Полный diff и `git diff -w -- crates/localvox-light-api` просмотрены:

| Файл | Изменение оформления |
|---|---|
| [build.rs](../../../../crates/localvox-light-api/build.rs) | Перенос длинного условия создания placeholder; HTML-строка сохранена |
| [localvox-api.rs](../../../../crates/localvox-light-api/src/bin/localvox-api.rs), [localvox-mcp.rs](../../../../crates/localvox-light-api/src/bin/localvox-mcp.rs) | Многострочные атрибуты command с прежними значениями |
| [archive.rs](../../../../crates/localvox-light-api/src/archive.rs) | Перенос цепочки чтения версии/маппинга реплик и условия/подписи recording; поля и значения сохранены |
| [chat.rs](../../../../crates/localvox-light-api/src/chat.rs) | Оформление closure, массива фраз и assert; выражения/тестовые строки сохранены |
| [http.rs](../../../../crates/localvox-light-api/src/http.rs) | Перенос предиката фильтра неизвестных/retired настроек, без изменения условия |

Изменения Rust: 6 файлов, 67 добавленных / 22 удалённых строки оформления.
`diff -w` не пуст из-за переноса строк, trailing comma и формы closure; это
не выдаётся за побайтовую или AST-идентичность. Проверка отсутствия изменения
поведения — code-review diff, дополненный существующими тестами ниже.

## Проверки

Локальные логи: target/api-format-*. Cargo: `--locked -j 2`, тесты с
`-- --test-threads=2`. Новые тесты для механического форматирования не создавались.

| Команда | Результат |
|---|---|
| `cargo fmt -p localvox-light-api -- --check` | PASS после прежнего FAIL |
| `cargo test -p localvox-light-api -- --test-threads=2` | 62 passed: 59 lib и 3 integration (hybrid RRF, journal HTTP, semantic overload); 6 ignored, ошибок нет. CLI test targets и doc tests также собраны |
| `openspec validate --specs --strict --no-interactive` | PASS: 15 specs; информационные замечания о длине требований |
| `openspec validate format-api-rust-sources --strict --no-interactive` | PASS, CLI 1.13.2 |
| `python scripts/check-openspec.py` | PASS: 15 capabilities, 86 требований, 168 сценариев, 86 строк main coverage |
| `git diff --check` и новые текстовые файлы | PASS |

Шесть ignored в обычном прогоне: ffmpeg/FLAC, историческая временная копия,
совместный API/UI/TUI, его helper, реальный media backend и settings helper.
Settings helper вызывается своим родительским тестом; остальные native/исторические
проверки этим запуском не объявляются выполненными. Модель для semantic overload
подставная и намеренно не отвечает; проверено сохранение доступности обычных HTTP.

В [interface CI](../../../../.github/workflows/interface-tests.yml) добавлены
rustfmt component и format check API до дорогих native/browser тестов, а также
строгая валидация этого change. Локально проверяется ровно команда из workflow.
Удалённый GitHub Actions в этом проходе не запускался.

## Границы

Контракты API-001–005, ARCH-001/003/005, CHAT-001/002, CFG-001–003 и PIPE-008/011,
с которыми связаны эти исходники, сохраняются. Форматирование не закрывает известные
RV/BL, включая controlled cancel/delete, и не заменяет их специальные сценарии.
Предыдущие результаты e2e/UI/TUI не переименовываются в новые прогоны; интерфейс,
snapshots и TUI в этом срезе не менялись и отдельно не перепроверялись.

Ни пользовательский архив, ни .env, ни работающий демон не изменялись. Реальные
модели/микрофон/звук не запускались; пункт 10.2 общего журнала остаётся not-run.
Main specs не синхронизировались, changes не архивировались. Коммит и индекс
по-прежнему под управлением пользователя.
