# Проверки настроек — 2026-11-02

Статус реализации и границы прогонов: [отчёт](implementation.md).

| ID | Код | Существенная проверка | Результат |
|---|---|---|---|
| CFG-001 | [presets](../../../../crates/localvox-light-core/src/presets.rs), [env editor](../../../../crates/localvox-light-core/src/env_file.rs), [API](../../../../crates/localvox-light-api/src/http.rs) | 3 presets, 15 env_file, 11 CLI и [настоящий settings HTTP](../../../../crates/localvox-light-api/src/http/tests/settings.rs): внешний preset, override, reset, неуспешное сохранение, секреты | test-run; пользовательский `.env` и демон не менялись |
| CFG-002 | [каталог/apply](../../../../crates/localvox-light-core/src/settings.rs), [coordinator](../../../../crates/localvox-light/src/autocook.rs), [worker](../../../../crates/localvox-light-asr/src/bin/localvox-process.rs) | 4 settings, settings HTTP: reset возвращает preset, snapshot требует Restart, пустые устройства не подменяют прежний выбор | test-run; физический capture not-run |
| CFG-005 | [API](../../../../crates/localvox-light-api/src/http.rs), [UI](../../../../ui/src/Settings.tsx), [TUI](../../../../crates/localvox-light-terminal/src/app/forms.rs) | HTTP round trip/dedup/reject credentials, [browser e2e](../../../../ui/tests/e2e/workspace.spec.ts), [TUI draft/no-op/palette](../../../../crates/localvox-light-terminal/src/app/ux_tests.rs) | test-run: add/select остаются черновиком до Save, повторное чтение сохраняет список; активный URL требует Restart |

CFG-004 уже занят управлением постобработкой в другом change и сохраняет прежний ID.
