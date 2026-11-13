# Voice grouping clock follow-up · 2026-11-13

Контракт PIPE-011 прежний; исправлена только точность тестового времени во втором
сценарии, не production и не общий workspace fixture.

## Hosted 99bd709

[Interface run 43477210896](https://github.com/cascad/localvox-light/actions/runs/43477210896),
Windows job 132064030006: UI build/unit passed, e2e **80 passed / 1 failed**.
`voice grouping stays active after all audio was read and retains its measured time`
ждал `0:05`; первый опрошенный DOM уже показывал `0:06`, затем вырос до `0:12`.
Ранее исправленный pipeline timing сценарий прошёл. Дальнейшие Rust/shared/spec
шаги skipped; предыдущий fee3103 Interface целиком passed, но это другой commit.

## Reproduction and correction

До fix временная Node задержка 1500 ms после workspace readiness воспроизвела
тот же пропуск `0:05`. Все assertions сохранились: **failed**, clock уже 7–12 с.
Trace/context: `target/tmp/grouping-clock-red-artifacts/`, лог `grouping-clock-red.log`.

С тем же probe `workspace(time=06:00:25)` и `pauseAt(07:00:25)` после navigation:
**passed** (3.1 s suite). Время продвигается только существующими runFor:
running `0:05` → `0:07`, после operation_end сохраняется measured `0:03`.
Состояние running при всём прочитанном аудио и проверки single running stage прежние.

Задержка после probe удалена. Sleep, retries, ослабления elapsed assertions,
обновления baseline и production изменений в итоговом diff нет.

## Local verification

- UI build **passed**, unit **15 passed**.
- Полные e2e **81 passed** (27.6 s, workers=2), PNG baseline не обновлялся,
  negative visual detector символа/цвета/геометрии прошёл.
- Оба точных timing сценария повторены по четыре раза: **8 passed** (10.5 s).
- Strict main/change, check-openspec, actionlint и diff **passed**.

Логи: `target/tmp/grouping-clock-{green,ui-build,ui-unit,ui-e2e,repeat}.log`.
TEMP и artifacts на D:, существующий Chromium, без пользовательского архива,
.env, моделей, демона и устройств. User-guide не меняется: поведение прежнее.

Новый hosted запуск исправленного теста — **not-run** до push владельца.
Состояние native [Build 43477210846](https://github.com/cascad/localvox-light/actions/runs/43477210846)
отслеживается отдельно; успешный локальный browser прогон не делает его green.
