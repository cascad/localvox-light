# Pipeline clock fixture, 2026-11-13

## Hosted observation

Владелец отправил `d30727b6b40252ba2f798ee5143f3182308eea46` в main.

- [Build 43429701723](https://github.com/cascad/localvox-light/actions/runs/43429701723):
  **success**, все пять Windows x64 / Linux x64+ARM64 / macOS Intel+ARM64 jobs.
  Packaging, startup/install/doctor/reconcile/registration checks passed;
  publish skipped для обычного push, bundles в workflow artifacts.
- [Interface 43429701720](https://github.com/cascad/localvox-light/actions/runs/43429701720):
  **failure**, Windows job 131899835533, шаг 9 `Build and test UI`.
  UI build и 15 unit passed; e2e **80 passed / 1 failed**. Тест PIPE-011
  ждал `Ожидание: 5 с`, но живые часы дошли до `12 с` (artifact 14039321483,
  error-context.md, trace.zip). Дальнейшие Rust/shared-client/media/spec шаги
  **skipped**. Предыдущая причина shared helper не подтверждена и не закрыта.

Artifacts скачаны в `target/tmp/interface-evidence-43429701720/`.

## Reproduction and correction

Старая fixture устанавливала clock на 07:00:25, не останавливая его.
При `since=07:00:20` точные 5 с доступны лишь до следующей секунды.
Production счётчик растёт правильно; требование PIPE-011 не меняется.

На исходный timing сценарий временно добавлена задержка Node 1500 ms после
workspace readiness, перед открытием «Ход». Все assertions остались прежними:
**failed** на тех же 5 с, в DOM уже 13 с после ожидания assert timeout.
Лог `target/tmp/pipeline-clock-red.log`; trace/context сохранены отдельно в
`target/tmp/pipeline-clock-red-artifacts/`.

Fix: только этот сценарий запускает часы на час раньше, затем после navigation
вызывает штатный pauseAt(07:00:25). runFor/fastForward управляют временем явно.
Default остальных сценариев прежний. После outage учитывается capped 30 s
polling backoff: виртуальный шаг 30.1 s вместо 3.1 s. Реальные timeouts,
assertions и PNG baseline не менялись.

Первая попытка с одним pauseAt прошла точные 5/7 с и скриншот, затем выявила
зависимость recovery от автоматического хода часов при backoff. После явного
шага 30.1 s тот же probe с задержкой 1500 ms **passed** целиком (4.9 s test,
6.0 s suite), `target/tmp/pipeline-clock-green-probe.log`.
Временная задержка удалена из исходников; sleep/retries/новых baseline нет.

## Verification boundaries

Пользовательские .env/archive/демон не затрагивались. Capture/модели не запускались.
TEMP/cache/test artifacts на D:, использован существующий Chromium.
UI поведение не менялось, user-guide не требует изменений.

## Final local checks

- UI build passed; 15 unit passed без skips.
- Полная Playwright suite: **81 passed** (29.6 s), workers=2,
  updateSnapshots=none. В том числе прежний pipeline-queued PNG и negative
  detector tests цвета/символа/геометрии. Baseline не обновлялся.
- Дополнительно flaky сценарий повторён четыре раза с workers=2:
  **4 passed** (10.8 s), retries отсутствуют.
- Strict main specs, strict обоих затронутых changes, check-openspec,
  actionlint обоих workflows, diff passed.

Логи: `target/tmp/pipeline-clock-ui-{build,unit,e2e}.log`,
`pipeline-clock-repeat.log`, `pipeline-clock-specs.log`.
Попытка повторно запустить только failed GitHub job через connector получила
403 `Resource not accessible by integration`: Actions write permission
у подключения отсутствует; новый run этим вызовом не создан.

Новый hosted run: not-run, нужен push владельца. Общий CI ещё не объявляется green.

## Hosted follow-up on 873ffa2

[Interface 43436838950](https://github.com/cascad/localvox-light/actions/runs/43436838950)
прошёл UI build, 15 unit и все 81 e2e, включая исправленный clock scenario.
Общий workflow позже failed на native shared UI/TUI helper, не на browser clock.
Root/отдельный fix — [Windows audio apartment lifetime](../../repair-shared-client-ci-fixture/evidence/windows-audio.md).
