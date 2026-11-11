# Design

## Context

См. proposal и исходный native-reference в add-incremental-recording-transcription.
Подтверждены 714 одинаковых ASR calls, 24 loads против одного; модель 224893347 bytes.
Каждый prefix заново fingerprint/load/fingerprint и многократно restore/derive/cache
publish. Вклад каждого остаточного участка wall ещё не изолирован.

## Goals / Non-Goals

**Goals:** model reuse без потери process isolation, fairness, cancellation и cache correctness.

**Non-Goals:** ASR внутри daemon, постоянное владение записью, внутренний второй job
queue, доверие mtime вместо SHA, новый ASR/LLM, изменение окон/порогов или ослабление gate.

## Decisions

- Stdio request/reply через owned process supervisor: один bounded JSON request и
  один ответ с совпадающими protocol/id; stderr отдельным ограниченным spool.
  Deadline/cancel проверяются во время write/read; нет бесконтрольных pipe-drainer
  threads, listening ports, файловых очередей или polling IPC. Любая protocol ошибка
  завершает дерево; следующий запрос начинает чистый процесс. Preflight version bump.
- Transcribe lane хранит optional worker и не меняет порядок claim. Processing lease
  принадлежит только текущему job. Request обслуживает prefix или final; engine
  хранит одну identity и не удерживает session/audio locks между запросами.
- Core AsrRuntime повторно использует engine только при совпадении fresh full content
  fingerprint. Изменение модели/языка/рецепта заменяет engine. Legacy one-shot API
  остаётся с локальным runtime; explicit force сохраняет повторный ASR.
- Keep-alive — LOCALVOX_ASR_KEEP_ALIVE_SEC из .env.defaults: 600 s перекрывает обычные
  300 s seals; 0 сохраняет одноразовый запуск. Idle/shutdown/cancel/error закрывают
  процесс целиком; память не растёт числом моделей/сессий. Значение видно в settings.
- Cache records перечитываются и проверяются на границе запроса/commit. Внутри одной
  операции Accepted batch проходит derive без повторного disk lookup и передаётся
  checkpoint/publication; нет process-wide доверенного кеша пользовательских файлов.
  Result commit/atomicity и final PCM verification остаются прежними.
- Отдельные atomic temp/rename на каждое окно заменяются framed append journal
  производных результатов: ключ/длина/checksum, ограниченные сегменты, fsync каждого
  результата до принятия и ссылки из прежнего checkpoint. Незавершённый/повреждённый
  frame пропускается при recovery без потери следующих валидных frames. Legacy
  windows/*.json читаются как fallback; массовой миграции/удаления нет. Новая схема
  сокращает create/rename, не снимает durability. Frame/segment/input bounds и
  corruption/tail/rotation/failure/legacy tests обязательны.
- Existing synthetic fixture paths по умолчанию могут использовать one-shot; добавляются
  реальные resident fixture tests с тем же worker server. Сравниваются оба пути.
- Успешный PCM range каждого окна относится к debug diagnostics, не к отдельному
  persisted journal log (он переписывает служебную metadata). Начало/конец порции,
  оконные progress counters и точный range в контексте ошибки сохраняются. Счётчики
  обновляются не чаще раза в секунду, обязательно на входе/выходе порции; это
  диагностический поток, а не подтверждение результата. Fsync результатов не меняется.

## Risks / Trade-offs

- Зависший/умерший child, обрыв frame, лишний stdout → bounded supervisor tests,
  kill/reap потомков, fail attempt без выдачи неподтверждённого успеха.
- Sticky owner/engine mismatch → same-generation/replacement/cancel/restart integration,
  один engine, fresh fingerprint и отсутствие session leases между requests.
- Дополнительная resident RAM → idle release/0 и замер high-water; не обещать слабый Mac.
- Удаление лишнего чтения скрывает corruption → читать/validate authoritative records
  при принятии, fault tests для checkpoint/result/PCM, immutable batch только внутри
  одного held execution lease. Гарантии durability не снимаются ради скорости.

## Migration Plan

Согласованно собрать daemon/worker с новым worker protocol. Несовместимый worker
не начинает очередь. Откат keep-alive=0 возвращает one-shot без изменения артефактов.
Benchmark: исходные whole PCM/weights/recipe, три alternating пары, 300/900/300 s,
совпадение coverage/text/calls и старый бюджет ≤20%; idle исключён как раньше.
Предыдущие +48.12% остаются историческим результатом. Реальные capture/Mac not-run.

Дополнительное измерение фиксируется отдельно в
[current-capture-plan](evidence/current-capture-plan.md): у нынешнего приложения
capture/planner работают и без раннего ASR, тогда как прежний O стенда — legacy.
CLI-флаг добавляет контроль с одинаковым capture, не заменяет исходный gate и не
переключает default. Оба результата публикуются независимо.
