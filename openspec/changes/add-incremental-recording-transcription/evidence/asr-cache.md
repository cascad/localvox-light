# Проверяемый STT cache, 2026-11-10

Этап 3.1, часть результата R1. [Horizon/identity](../../../../crates/localvox-light-core/src/incremental.rs),
[cache/checkpoint](../../../../crates/localvox-light-core/src/incremental/cache.rs),
[tests](../../../../crates/localvox-light-core/src/incremental/tests.rs).
Worker/coordinator/API ещё не подключены к этому модулю; ранний текст пользователю
пока не доступен. Это не проверка нативного качества ASR или полного пути 3.2.

## Контракт принятия

Horizon фиксирует plan snapshot и проверяет generation/revision, sealed source
ranges, последовательность/EOF и checksum каждого immutable descriptor. Открытый,
unknown, переставленный/неучтённый диапазон не проходит. JSON читается с конечными
лимитами; recipe/PCM fingerprints не используются как произвольные файловые пути.

Identity включает engine/language/ASR recipe/revision и SHA фактических model/vocab
dependencies. Чтение файлов потоковое и отменяемое; путь/mtime не заменяют содержимое.
В `incremental/<identity SHA>/identity.json` сохраняется описатель fingerprints.
Ключ окна содержит эту identity и полный descriptor: generation/source/start/end,
planner recipe, PCM SHA, speech/silence kind. Контексты другой модели не смешиваются.

`VerifiedWindow` требует точного sample count и SHA перед lookup или вычислением.
Валидный cache hit не вызывает ASR и не перезаписывается. Silent окно не вызывает
ASR; speech с ответом без допустимых слов имеет отдельный Empty outcome. Ошибка
декодера/ASR/publish не становится Empty. Raw answer и отфильтрованный text хранятся
раздельно; тот же прежний `has_real_word` перенесён в ASR common module без изменения
алгоритма, публичный cook re-export сохранён.

Record атомарно сохраняется/sync-ится через общий writer, перечитывается/проверяется
и только затем может попасть в checkpoint. Оба имеют checksum envelope. Битый или
неполный derived record пересчитывается; I/O отказ чтения не маскируется под cache
miss. Как у VersionStore, запись cache требует execution ownership у вызывающего
контура: standalone CLI держит lease, managed worker покрыт lease координатора
на весь срок дочернего процесса. Отдельного конкурирующего замка cache не создаёт.

Checkpoint ссылается только на непрерывные prefix records каждой дорожки. При
повреждении suffix откатывается до последнего доказанного окна; независимая дорожка
и отдельные валидные window files сохраняются. Counters/end/eof из saved checkpoint
не принимаются на веру: границы выводятся из referenced records и текущего horizon.
Result, сохранённый перед crash без checkpoint, не продвигает prefix сам по себе;
его можно принять после нового VerifiedWindow без повторного ASR.

Существующий подтверждённый prefix переносится между horizon по checksum/полным
зависимостям без повторного PCM/VAD прохода всего архива на каждом UI чтении.
Переиспользование окна worker через resolve требует фактический VerifiedWindow;
проверка полного PCM при финальном cache reuse относится к 3.2. Read checkpoint
не означает `best`, готовность полного transcript receipt или разрешение summary.

Common coverage = минимум неоконченных source frontiers. Publication watermark
считает порядок start/source, не отбрасывает более длинное готовое окно только из-за
его end: иначе более ранний start мог бы вставиться после уже показанной реплики.
Дополнительно публикация ждёт доступного аудио всего окна. `publication_end` может
быть меньше `common_end`; поле не выдаёт незаконченный хвост за готовый.

## Выполненные проверки

Восемь model-free tests используют настоящие temporary recording seals/WAV, явно
заданные descriptor ranges, синтетические weights/vocab и ASR callback counters:

- Crash после полного result и до checkpoint: prefix не продвинут, reopening
  перепроверяет PCM и принимает result без нового ASR. Повтор EOF/checkpoint не
  дублирует строки; silent/empty outcomes закрывают покрытие без текста.
- Повреждение среднего record: только его source prefix откатывается; при полном
  проходе повторный ASR вызван ровно один раз, остальные окна — cache hits.
- ASR timeout, refusal result publish и checkpoint publish: нет ложного progress,
  предыдущая state сохранена; новый synced result остаётся переиспользуемым.
- Complete/synced temporary bytes без atomic publish игнорируются. Это состояние
  файлов между шагами commit, не физическая эмуляция потери питания/отказа fsync.
- Изменение PCM/count, модели, vocab, language, recipe, generation, range или planner
  identity запрещает неверный hit. Missing/cancelled model dependency отказывает.
- Неравные окна двух дорожек сохраняют порядок опубликованного prefix, ожидают
  доступного аудио и после EOF дополняются без вставки ранней реплики задним числом.
- Forged frontier/eof не двигают подтверждённое; foreign/stale horizon не commit-ится.
  Corrupt descriptor и checksum-consistent invalid Empty outcome не принимаются.

Команда: `cargo test --locked --offline -j 2 -p localvox-light-core --features onnx --lib --quiet`,
TEMP/TMP=`D:\rust_repo\localvox-light\target\tmp`. Последняя полная серия: core
441 passed, 7 ignored; API 79 passed, 6 ignored; application/worker/TUI check passed.
В ходе прогона найден и отдельно исправлен [Windows Stop publish defect](atomic-stop.md),
включая воспроизведение 63/64 до исправления и 64/64 после. Исторический неуспешный
прогон не заменён успешным. Installed release/daemon/private archive не менялись.
Новые cache tests включаются в CI как model-free; запуск CI здесь не заявлен.

PIPE-015/016 delta: проверены horizon/cache/checkpoint/frontiers; полноценный worker,
Stop→final receipts/diarization и fault process fixture остаются в 3.2. Принятый
PIPE-004: прежний noise filter сохранён, regressions проходят. Native модели,
reference budget, UI/TUI и hardware этим этапом не проверены.
