# PIPE-019 · Реализация и проверки, 2026-11-11

Изменение реализации раннего STT, не принятие default on. Main specs не
синхронизированы; PIPE-019 остаётся delta. Исходный R4 +48.12% сохранён как
исторический результат. [Новая native серия](native-reference.md) отдельно
проверяет прежний бюджет; [промежуточные замеры](experiments.json) не скрыты.

## Поведение и границы доверия

Transcribe lane последовательно обслуживает prefix/final через один owned stdio
процесс. Между запросами остаётся только один ASR engine; session/execution leases,
PCM и принятые cache batches не удерживаются. Полный fingerprint проверяется заново
на запрос; замена identity освобождает прежний engine до загрузки нового. Force
по-прежнему распознаёт заново. `LOCALVOX_ASR_KEEP_ALIVE_SEC=600`; 0 возвращает
one-shot, idle/shutdown/error/cancel освобождают процесс. Defaults, каталог настроек
и US-055 обновлены. Нужен согласованный комплект `localvox-worker/3`.

Супервизор владеет деревом процесса: bounded 64 KiB request / 8 MiB reply,
protocol/increasing id, deadline/cancel, контролируемый diagnostic spool и очистка
при любой ошибке обмена. stderr предыдущего запроса не переходит в следующий;
idle diagnostic flood проверяется при проверке живости. Windows Job Object /
Unix process group сохраняют прежние правила дочерних инструментов.

Результат каждого окна синхронизируется до принятия. Ограниченные framed segments
с header/payload SHA заменяют отдельные temp/flush/rename; partial/corrupt frame
не принимается и не скрывает следующие целые результаты. Legacy JSON читается
без миграции. При открытии запроса records перечитываются; внутри held execution
lease используется один verified batch. Новые records повторно проверяются перед
checkpoint. Current pointer публикуется последним; final проверяет actual PCM.
Ни checksum, ни fsync, ни проверка первичного аудио не убраны ради скорости.

Начало/итог порции и оконные счётчики остаются в журнале. Счётчики объединяются
до одного обновления в секунду с обязательным входом/выходом; PCM range каждого
окна — debug и контекст ошибки. Ошибка prefix/load помечает вложенную операцию
ошибкой и не публикует успешный checkpoint.

## Test-run

TEMP/TMP на D:, Cargo `--jobs 2`, `INSTA_UPDATE=no`.

- Core: 460 passed / 9 ignored, 2 fbank integration passed. Фокусный cache прогон:
  corruption/checkpoint/PCM/identity/descriptor, suffix rollback, atomic publication,
  legacy hit без ASR/миграции, transaction revalidation, WAL checksum/оборванный
  payload/повторное открытие/rotation/limit/write refusal.
- ASR worker: 5 integration passed. Настоящие child processes проверяют crash
  после result/checkpoint/final commit, отсутствие duplicate ASR/версий, force и
  legacy, два горизонта → смену identity → final в одном PID, освобождение leases,
  protocol error/repeated id и failed prefix без fake success.
- Process: 12 lifecycle passed / 1 helper ignored; custom resident IO harness passed:
  два ответа/один PID, раздельные diagnostics, EOF/timeout/cancel, input/stdout/stderr
  limits, idle flood и завершение descendant после timeout.
- Coordinator: 15 unit passed / 11 ignored, включая 0/idle expiry/clear ownership.
  Отдельно explicit fixture worker: finite-prefix coordinator integration passed —
  growth/Stop/disable/cancel/retry, deadline/fault/missing/changed suffix preservation.
  Это настоящий coordinator/capture/planner/worker/API в temp, без моделей/устройств.
- API: 81 unit passed / 8 ignored; RRF, journal cursor и overload integration passed.
- Terminal: 190 lib / 5 actual ConPTY / 2 IO / 2 run-script passed; 3 helpers и
  hardware mpv ignored. Оформление и его baselines — отдельный change.
- UI production build passed; physical-key unit/browser доказательства предыдущего
  этапа — [keymaps](../../configure-command-keymaps/evidence/implementation.md).

Команда общего прогона: `cargo test --locked -p localvox-light-core -p localvox-light-asr -p localvox-light -p localvox-light-api -p localvox-light-process -p localvox-light-terminal --features localvox-light-asr/asr-fixtures --jobs 2 -- --test-threads=2`.
Логи: ignored `target/tmp/warm-asr-verified-{regressions,coordinator,cache}.log`.
Изменения style snapshots проверены отдельно; первоначальный ожидаемый style diff
не выдан за пройденный тест. После review styles повтор с `INSTA_UPDATE=no` прошёл.

## Not-run

Аппаратные microphone/loopback, физический звук/mpv и слабый Mac остаются not-run.
Native series проверяет GigaAM и равенство с offline, не независимое качество речи,
diarization или LLM. Личный архив и работающий пользовательский демон не запускались.
Рефакторинг всего проекта не начат. Push не выполнялся.

## Release и результат измерения

Согласованные daemon/worker/UI/TUI собраны; worker protocol `localvox-worker/3`,
TUI `--version` и daemon `--help` прошли. SHA и путь review — в [bundle](bundle.json).
`target/release/localvox-tui.exe` и `target/review/vim-navigation/localvox-tui.exe`
совпадают; пользовательский daemon не запускался.

Legacy comparator: aggregate +18.51%, первые две пары >20%, итог **fail**.
Дополнительный заранее зафиксированный current-capture comparator: **все три пары
pass, aggregate +2.95%**, то же PCM/weights/recipe/20% и полная parity. Он учитывает
нынешние capture/planner у обеих сторон и не заменяет исходный gate/аппаратную приёмку.
Модель загружается один раз; готовые окна не распознаются заново. Ранний default off.

После замены фиксированного ожидания idle diagnostic test на bounded polling
custom resident IO harness повторно прошёл. Производственный код после native
серий не менялся.

Strict main: 15 passed / 0 failed; strict change valid; `check-openspec.py` PASS
(15 capabilities / 88 requirements / 188 scenarios / 1854 local links). Diff reviewed;
secrets, `.env`, models, public PCM и generated target outputs в коммит не входят.
