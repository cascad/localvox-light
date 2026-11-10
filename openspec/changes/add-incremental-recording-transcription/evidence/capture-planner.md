# Capture-owned planning, 2026-11-10

Этап 2.2. [Planner/ownership](../../../../crates/localvox-light-core/src/chunks/planning.rs),
[recorder notification](../../../../crates/localvox-light-core/src/chunks/capture.rs),
[capture integration](../../../../crates/localvox-light-core/src/pipeline.rs).
Это подготовка входных окон, без ASR/LLM, checkpoint результата, early UI/TUI,
финализации из кеша и нового включённого default STT. Остальные задачи остаются открыты.

## Вход, публикация и время

Один native WebRTC VAD LowBitrate и общий Windower на источник создаются в управляемом
потоке. Recorder после успешного atomic seal посылает unit wakeup в bounded(1) канал.
Ревизия читается с диска; новые notifications объединяются, PCM не ставится в очередь.
Idle timeout 250 ms проверяет shutdown без дискового polling. Второй planner тому же
writer отказывает; путь записи берётся из владельца, его нельзя подменить параметром.

Каждый новый sealed файл читается под общим WAV/FLAC lease блоками до 64 KiB, с точной
проверкой count. FLAC требует проверенную conversion receipt; decode и проверка SHA
отменяемы. VAD хвост/неполный frame сохраняются между файлами и pause. Только primary
terminal позволяет finish. Unknown останавливает соответствующий source frontier,
но другая дорожка продолжает подготовку. Новая поздняя/отключённая/неопределённая
дорожка не становится тишиной: общий минимум ждёт её или подтверждённого EOF.
Существующая аудиошкала/pre-roll не заменены эвристической синхронизацией.

`capture/plan/<recipe SHA>/srcN_<start>-<end>.json` — immutable descriptor:
generation/source/sample bounds, speech/silence kind, PCM SHA, recipe fingerprint.
Идентичный replay принимается, несовпадающее содержимое не перезаписывается.
После descriptors атомарно публикуется `capture/plan/state.json` со ссылками/SHA,
capture revision, sequence/input_end/closed_end/eof на источник, common_end/complete.
Слова и ASR receipt здесь отсутствуют. Повреждённый descriptor не является результатом;
recovery/инвалидация ASR output — следующие этапы, не доказательство этого прогона.

При Stop session handle только сигналит последний horizon, не join-ит capture thread.
Stopped читается до disk snapshot: гонка Stop не позволяет выйти до последней ревизии.
Поток держит Writer Arc/capture lease до конца, в том числе после recording marker.
Manager владеет максимум двумя потоками; перегрузка не мешает primary recording.
Shutdown отменяет и join-ит их, без ложного error на ожидаемую отмену. Отказ input,
descriptor или state publication заканчивает planner с capture error; аудио пишется
дальше. Полная обработка не зависит от наличия плана. Без аппаратного/дискового сбоя
тестовые shutdown границы выдержаны; зависание самого системного fsync не объявляется
устранённым таймером Rust. RV-014 этим этапом полностью не закрыт.

## Реально выполненные проверки

[Model-free tests](../../../../crates/localvox-light-core/src/chunks/planning/tests.rs):

- Native VAD, произвольная нарезка закрытыми WAV, неполные frames и pause: все
  descriptors/PCM SHA совпали с непрерывным общим planner, EOF хвост сохранён.
- Старый WAV временно вынесен после первого advance: следующий horizon успешно
  прочитал только новый файл; перед EOF исходник возвращён. Нет повторного decode
  всего префикса. Late/uneven source ограничивает common_end; unknown не склеивается.
- Повторный planner принимает идентичные immutable descriptors; изменение PCM
  вызывает отказ и не заменяет последнюю принятую state.
- 20 seals + 2000 повторных wakeups coalesce; Stop без нового PCM достиг полного
  последнего horizon. Повторный attach отклонён. Два idle planner ограничивают pool,
  третий отказ не мешает записи; после освобождения ресурса attach проходит.
- Refused state publication пишет error; затем primary WAV/terminal сохраняются.
  Idle shutdown не создаёт EOF/error; удержанный chunk lease не позволяет отпустить
  capture ownership до выхода управляемого потока.
- Настоящий FFmpeg: два verified FLAC через sample seam дают тот же полный plan,
  что WAV/непрерывный PCM; проверка conversion SHA принимает cancellation.

[Capture pipeline](../../../../crates/localvox-light-core/src/pipeline.rs):
pre-roll 1 s/0.5 s, pause, обе дорожки и Stop дошли до plan complete с точными
16320/8640 samples и generation, без versions/processing artifacts.
[API mutation](../../../../crates/localvox-light-api/src/archive/mutations.rs):
реальный planner владеет закрытой записью при занятом chunk lease; delete после Stop
отказывает до изменения jobs/files. После shutdown освобождение проверено, delete
проходит. Это вызов настоящего API domain, не новый HTTP/аппаратный прогон.

Команды из корня (TEMP/TMP на D:, без рабочего демона/архива/моделей):

```powershell
cargo test --locked --offline -j 2 -p localvox-light-core --features onnx --lib --quiet
cargo test --locked --offline -j 2 -p localvox-light-api --lib --quiet
cargo test --locked --offline -j 2 -p localvox-light-core --lib chunks::planning::tests::real_ -- --ignored --nocapture
```

Последний прогон: core с ONNX — 431 passed, 7 ignored; API — 79 passed, 6 ignored;
real FFmpeg planner — отдельно 1 passed. Strict main: 15 passed / 0 failed;
change valid; check-openspec: 88 requirements, 188 scenarios, 88 coverage rows,
1629 local links PASS. Rustfmt/diff clean; application/worker/TUI cargo check passed.
Новый real FFmpeg test добавлен в CI; запуск CI этим не заявлен. Capture/codec
обычные тесты уже входят в CI chunks filter. Нет нового тестирования STT качества,
CPU reference budget, длительной записи на микрофоне/loopback, Unix или слабом Mac.

## Traceability

| Требование | Что подтверждено на этом этапе |
|---|---|
| STORE-004 delta | Только sealed ranges; unknown/late source fail closed; lease/Stop ownership |
| PIPE-015 delta | Native непрерывные окна через файлы; per-source/common input frontiers, без ASR результата |
| PIPE-016 delta | Immutable input descriptors/PCM SHA; ASR cache/checkpoint/final acceptance ещё not-run |
| REC-003, INV-REC-001, INV-REC-002, REC-005 | Синтетический pre-roll/pause/Stop без нового PCM, shutdown planner; hardware not-run |
| ARCH-005, INV-SESSION-001 | Delete не обходит живого primary planner после Stop |
| STORE-002 | Отменяемая проверка conversion SHA; настоящий FLAC seam; прежние границы RV-005 сохранены |
