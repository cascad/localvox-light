# R2 · Очередь и управление, 2026-11-10

[Queue intent](../../../../crates/localvox-light-core/src/jobs/prefix.rs),
[coordinator](../../../../crates/localvox-light/src/autocook.rs),
[immutable publication](../../../../crates/localvox-light-core/src/incremental/publication.rs),
[real fixture](../../../../crates/localvox-light/src/autocook/prefix_tests.rs).

Одно Incremental задание хранит последний frozen input, accepted work key и Stop
intent. После конечной порции оно становится WaitingInput без worker/model/lane.
Новые seals без новых descriptors не запускают ASR. Новые окна обновляют одно
задание, включая во время исполнения; claimed copy остаётся фиксированным.
Обычные задания получают приоритет перед следующей ранней порцией той же STT линии.
На Stop running kind не меняется: после его подтверждения тот же job переходит в
Cook/Prepare. Ошибка ранней порции не запрещает full fallback на подтверждённом Stop.

Cancel/Retry меняют только processing intent и могут сосуществовать с capture.
Cancel сохраняется tombstone и не обходится Stop/restart. Delete/lang/best/recook
сохраняют capture mutation fence. Раннее вычисление зависит от общего autocook:
выключенный coordinator не создаёт линий, лёгкий capture planner ASR не выполняет.
Live выключатель не убивает текущую порцию, запрещает следующую; Stop работает.
Видимый bool `LOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE` добавлен в общий settings schema.
В промежуточной ветке preset **off до R4**; обещанный on нельзя включать до полного
пути и reference budget. Это не завершение default gate 7.3.

## Worker protocol / ownership

Протокол повышен до `localvox-worker/2`. Новый комплект проверяется до claim;
старые daemon/worker не смешивать. Замена действующего установленного комплекта
не выполнялась. Старые evidence с protocol/1 остаются историческими.

Coordinator вне queue mutex фиксирует план в checksum-named `incremental/inputs/`,
после сохранённого claim под execution lease пишет маленький claims/job.json и
передаёт `--worker-prefix --worker-input`. Worker проверяет checksum/gen/revision,
читает только тот horizon. Новые закрытые файлы не расширяют текущее задание.
Snapshot text/checkpoint/primary ranges/plan записываются целиком в immutable file,
затем result receipt, последним current pointer. Верификация результата проходит
вне queue mutex; внутри — только сравнение execution id и сохранение перехода.
Raw PCM/model/LLM не оказываются под queue mutex.

## Выполнено

- `cargo test --locked --offline -j2 -p localvox-light --lib autocook::prefix_tests -- --ignored --nocapture`:
  **1 passed**, 3 сценария stop/disable/cancel. Реальные capture/planner/coordinator,
  managed child fixture worker и Archive API, временный каталог без устройств/моделей.
  Прогон 12.82 s. Чтение опубликованного prefix, отсутствие ранних versions/summary,
  без respawn неизменного horizon; рост, Stop/claim race, cancel при живом capture,
  запись после cancel, sticky Stop, явный Retry, final receipt. Для stop/disable
  SHA ASR-входов уникальны: принятые окна не распознаются повторно.
- 3 queue tests: coalescence/frozen claimed input, обычная очередь перед ранней,
  WaitingInput без новых окон, Stop передачи, cancel/reload/Retry, Live off.
- Publication test: frozen input остаётся доступен после замены текущего plan;
  text/checkpoint стабильны, Silence второй дорожки не даёт выдуманных строк;
  broken current не портит старый snapshot, changed immutable bytes отвергаются.
- Последний core без ONNX: **415 passed, 6 ignored**. Application library:
  **14 passed, 10 ignored**. Worker fixture серия после подключения frozen input/
  publication: **3 passed**. API: **79 passed, 6 ignored**. Первый API прогон выявил
  слишком широкий допуск Cancel старого Cook во время записи; допуск ограничен
  новыми capture/Incremental заданиями, прежние guards сохранены. Ожидание списка
  Live-настроек обновлено для нового bool. После исправления полный API прогон прошёл.

Нативные качество/скорость, двухчасовой reference, API media/Range и клиенты этим
срезом не проверены. Настоящий UI/TUI partial document относится к R3. Hardware/Mac
not-run; новые IDs остаются delta, incomplete change не sync/archive.
