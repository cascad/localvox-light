# BL-005: фоновый индексатор, worker и HTTP поиск — 2026-11-01

Проверено в исходниках после `c543b27`, Windows, dev profile, Rust `--locked -j 2`.
Это **native integration с синтетическим содержимым/embeddings**, не приёмка
реальной модели, слышимости или пользовательского архива. Работающий демон не
перезапускался. Установленный release этим прогоном не обновляется.

## Стенд и требования

[Native tests](../../../../crates/localvox-light/src/autocook/indexing_tests.rs)
запускают настоящий [coordinator/warm_indexes](../../../../crates/localvox-light/src/autocook.rs)
и sibling `localvox-process --worker-index`. Отдельный subprocess получает чистое
окружение и cwd нового временного каталога; loopback `/api/embed` выдаёт фиксированные
3D-векторы, 503 или удерживает запрос. Ни Ollama, ни ASR/NER, ни устройства не нужны.
TEMP/TMP направлены на D: в `target/test-temp`.

HTTP-запросы идут через настоящий Rust Client, tiny_http и production respond/Archive;
это частный тестовый сервер, без production listener/пула и доступа владельца.
Объект API обрабатывает запросы параллельно, поэтому проверяется реальный
SemanticGuard и файловый lock фонового worker, а не блокировка тестового handler.

| Требование | Native scenario | Фактическое доказательство |
|---|---|---|
| PIPE-013 | Pending → idle | Coordinator достиг ready, удерживаемое execution lock задание Pending не допускает старта. После синтетического завершения очередь освобождается; файлы Tantivy/semantic созданы **до** поискового запроса. |
| SEARCH-001 | Индексы и best | Lexical/semantic/hybrid находят transcript session+12.5 s; summary не получает выдуманный таймкод. Новый best пересчитывается настоящим index worker, новые записи проверены в semantic.jsonl до search; старый текст выбранной сессии отсутствует. |
| PIPE-013 | Новая работа и resume | На удержанном embed-запросе warm_indexes выполняется в настоящем процессе; новое Pending приводит к Cancelled и warm_indexes=false менее чем за 4 s после enqueue. После завершения тестовой работы повтор того же production warm_indexes создаёт индекс, HTTP поиск работает. Автоматическая переустановка dirty в coordinate отдельно сверена по коду; этот переход не выдаётся за новый захват/распознавание. |
| PIPE-013 | Timeout | Coordinator ограничивает worker 3 s, удержанный endpoint подтверждает исполняющуюся индексацию; после timeout нет горячего повторного запроса в idle. Исходные bytes и готовый transcript не меняются, Failed-job не появляется. |
| PIPE-013, SEARCH-002 | Ollama 503 / connection refused | Worker завершается управляемо, lexical/hybrid и чтение transcript/summary работают. Semantic возвращает ошибку после отдельного исправления RV-017. Пустой архив без ошибки остаётся пустым и не вызывает embed. |
| SEARCH-001/002 | Восстановление и старый кеш | После возврата endpoint строятся векторы. При отказе обновления нового best байты прежнего semantic.jsonl сохранены; после восстановления worker обновляет файл и поиск находит новый текст. |
| SEARCH-003 | Warmup + параллельные reads | Worker удерживает semantic lock; четыре HTTP semantic-запроса заполняют два разрешённых места, излишний быстро получает «занято», чтение transcript остаётся доступным. После снятия gate обычный поиск работает. |

## Выполненные команды

```powershell
$env:TEMP='D:\rust_repo\localvox-light\target\test-temp'
$env:TMP=$env:TEMP
cargo build --locked -j 2 -p localvox-light-asr --bin localvox-process
cargo test --locked -j 2 -p localvox-light --lib autocook::indexing_tests:: -- --ignored --skip indexing_fixture --test-threads=1 --nocapture
cargo test --locked -j 2 -p localvox-light-search -p localvox-light-api --lib -- --test-threads=2
cargo test --locked -j 2 -p localvox-light --lib -- --test-threads=2
```

- Worker build — PASS; preflight каждого fixture проверяет `localvox-worker/1`.
- Native — **5 passed**, 14.32 s; idle, cancel/resume, timeout, unavailable/recovery,
  concurrent warmup/search. Приватный ignored fixture исключён из счётчика командой
  `--skip indexing_fixture` и сам по себе не считается интеграционным сценарием.
- API — **76 passed, 6 ignored**; search — **11 passed**.
- Обычный coordinator lib, финальный прогон — **14 passed, 9 ignored**, 4.96 s. Native cases запускаются отдельно; обычный PASS не доказывает BL-005.
- Команды включены в Windows [CI](../../../../.github/workflows/interface-tests.yml)
  после сборки worker; удалённый CI в этой сессии не выполнялся.

Локальные логи: `target/bl005-native-final.log`, `target/bl005-regression.log`,
`target/bl005-coordinator-regression.log`, `target/bl005-worker-build.log`.

## Обнаруженное расхождение и границы

Первый прогон не был зелёным: scenario unavailable воспроизводил «успешный пустой»
semantic вместо ошибки (RV-017). Исправление вынесено в
[отдельный change](../../fix-empty-semantic-index-failure/proposal.md), а не скрыто
под skip_specs проверочного стенда. История red → green записана
[отдельно](../../fix-empty-semantic-index-failure/evidence/implementation.md).

В первых запусках стенда также наблюдались timeout старта/отмены, один supervisor
сообщил `process did not exit after termination`. Затем эти три сценария прошли
отдельно. Стенд теперь ждёт coordinator ready и даёт обычному индексированию 20 s;
целевой лимит worker составляет 3 s; стенд дополнительно подтверждает вход в endpoint.
Причина первоначальных задержек не установлена, эти отказы не переименованы в PASS
и не отнесены к исходному пользовательскому shutdown-инциденту RV-014. Лог первого
прогона сохранён в `target/bl005-native.log`; финальные результаты выше отдельные.

Фиксированные векторы проверяют транспорт, хранение и ранжирование заданного
совпадения. Они не подтверждают качество модели bge-m3/других моделей, полноту
реального архива, скорость большого индекса или аппаратную приёмку UI/TUI.

## Проверки формы

`cargo check --locked -j 2 -p localvox-light --no-default-features` — PASS.
Strict main specs — PASS (15 capabilities), strict обоих changes — PASS.
`python scripts/check-openspec.py` — PASS: 15 capabilities, 87 requirements,
175 scenarios, 87 coverage rows. `rustfmt --edition 2021 --check` для native fixture
и semantic.rs, `git diff --check` — PASS. Новые delta-сценарии пока остаются в change;
обязательные требования main не переписывались под дефект.
