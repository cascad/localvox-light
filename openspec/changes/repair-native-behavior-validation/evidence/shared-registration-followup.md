# Повторный Mac прогон: shared test не зарегистрирован · 2026-11-26

По предоставленному владельцем тексту отчёта `20261126T092746-13608`:
20 шагов passed, единственный failed — `gui-and-tui-real-api`. Python 3.13,
versions probes, GUI/browser, native PTY, archive/settings, installation и
реальный mpv прошли. Полный report.json этого повтора не получен, точный SHA
и состояние dirty tree не утверждаются.

## Подтверждённая причина

В `http.rs` модуль `cross_clients` оставался под `#[cfg(windows)]`.
На Mac фильтр `http::tests::cross_clients::shared_workspace` выбирал 0 tests,
102 filtered out. Runner корректно вернул failed для пустого Rust suite;
предупреждения deprecated/dead_code не являются причиной отказа.

Это отдельный пропуск тестового покрытия, а не доказательство поломки UI/API:
сценарий не исполнялся. Исправленная ранее канонизация путей оставалась внутри
этого platform gate и не могла сама исправить обнаружение теста.

## Исправление и регресс

- Модуль и его portable-pty/HTTP/Node fixture доступны на всех поддерживаемых ОС.
- Обычный API regression ссылается на shared function (случайный cfg-only gate
  даёт compile error) и проверяет через настоящий libtest `--list`, что точный
  externally selected ignored test действительно зарегистрирован.
- Собственный PTY получает TERM=xterm-256color независимо от терминала родителя.
- Unix native jobs ставят Chromium, проверяют регистрацию и исполняют реальный
  shared scenario с готовым release TUI. Linux устанавливает зависимости browser
  через Playwright; Mac использует штатный Chromium package. При отказе сохраняется
  native-shared artifact с browser/terminal traces. Windows gate также проверяет
  регистрацию до выполнения сценария.
- Не ослаблены assertions shared scenario, не скрыт 0-tests failure, не изменён
  установленный клиент или runtime behavior.

## Выполнено локально — Windows

- Настоящая регистрация test binary: 1 passed, 0.13 s,
  `target/review/shared-portable-discovery.log`.
- Настоящий браузер + TUI + private API/archive: 1 passed, 5.85 s,
  `target/review/shared-portable-real.log`.
- Runner и delivery workflow contracts: 23 tests, OK, 1 skipped
  (`test_actual_archive_doctor_setup_and_reconciliation` требует явно
  указанного полного native archive);
  `target/review/shared-portable-python.log`. Проверки YAML не считаются
  выполнением native CI jobs.
- `cargo fmt -p localvox-light-api -- --check`, strict validation change и
  всех specs, `scripts/check-openspec.py`, `git diff --check` — passed.
  OpenSpec checker: 15 capabilities, 88 requirements, 188 scenarios,
  88 coverage rows. Информационные замечания валидатора о длинных
  requirement texts не являются ошибками.

Новые native Mac/Linux shared-scenario runs и полный пользовательский Mac
повтор на момент этой записи ещё не выполнены. Приведённые выше 20 passed —
свидетельство владельца о предыдущем прогоне, не новый запуск агента.
Аппаратные capture/model сценарии этим не проверялись.

Команда повторения группы — [behavior testing](../../../../docs/behavior-testing.md).
