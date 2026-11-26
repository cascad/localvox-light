# Tasks

## 1. Capture lifecycle и macOS metadata

- [x] 1.1 Добавить штатные usage descriptions daemon Mach-O, проверку секции в native delivery gate и regression metadata; обновить справочник о macOS consent. Metadata parser прошёл на synthetic Mach-O; actual Mac link/consent — задача 3.2.
- [x] 1.2 Реализовать CaptureMonitor и signal/error lifecycle без диска в callback, guard macOS duplex input; проверить transitions/retry/quiet/disabled/restart и callback failure. 11 audio tests passed; publisher вынесен из capture packet reader.

## 2. Обратная связь клиентам

- [x] 2.1 Передавать capture_sources в /api/record только при живом engine; проверить реальный HTTP, stale state и совместимость старых ответов. Native private HTTP test passed.
- [x] 2.2 Показать источники в UI/TUI и actual failures в существующих консолях с recovery/dedup; покрыть state/render/browser, обновить US-037. TUI 262 passed, browser и StrictMode passed.

## 3. Интеграция и свидетельства

- [x] 3.1 Выполнить подходящие локальные Rust/Python/GUI checks, strict change/specs, check-openspec и diff; записать evidence/coverage и создать проверенный коммит. Результаты и границы — evidence/implementation.md.
- [ ] 3.2 На Mac проверить первый системный запрос, отказ/повтор после выдачи доступа и реальное сохранение звука другого приложения отдельно от микрофона. Без Mac hardware результата оставить not-run.
