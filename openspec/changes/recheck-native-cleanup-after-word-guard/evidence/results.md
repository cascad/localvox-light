# Fresh native cleanup после RV-020 — 2026-11-09

Обе модели прошли неизменённые короткие проверки. Длинные прогоны подтверждают
сохранность исходника/артефактов, но не дают приёмку автоматической правки всех
встреч: остаются неоднозначные unknown→known замены и спорная пунктуация.
Модель/default/.env приложения не переключались; автоматический маршрут остаётся
обязательной транскрибацией с опциональной summary.

## Scope and provenance

База `a744f9c`; worker из [предыдущей свежей release сборки](../../verify-current-release-pipeline/evidence/results.md)
не пересобирался и побайтово совпадает с её worker. Release profiler собран после
изменения только инженерного подключения. Ни prompt, guards, corpus, ни ожидаемые
ответы не менялись. Это новые native генерации, отдельно от прежнего replay.

[Числа и fingerprints](native-results.json), [выборка до native](selection.json),
[content-free review](public-review.json). Полные тексты/артефакты/логи сохранены
в `target/cleanup-recheck-20261109-a/` на D:, в Git не добавлены. Config hashes
остались в частном frozen.json; в evidence только результат проверки сохранности.
Ollama общая, benchmark не устанавливает эксклюзивное владение GPU, не выгружает
модели и не запускает daemon/capture/новую STT. Нет загрузок моделей или аудио.

Исходник — прежняя публичная STT: 704 строки, 7146.527375 s, raw SHA256
`09014305b713638301d3cd4693d03e1f7f2920e75bdfc749c7164d0e10034ae8`.
Каждый cleanup получает новую независимую v001 копию. Production artifact verifier
проверил все receipts; evaluator повторно проверил raw source/version/edits/выходной
SHA. Transcript receipt использует канонический Rust speech stamp, это не raw SHA.

## Direct profiler

Обычный `--text-only` теперь использует переданный endpoint напрямую; скрытый proxy
не создаётся. В отчёте `trace_enabled=false`, `llm_trace=null`: HTTP requests,
native tokens/durations/done_reason **не измерены**, не нули. Wall включает worker,
guards и запись; нельзя сравнивать его с чистым generation time старой серии или
объявлять отсутствие truncation. Sampling/context/caps production не менялись.
Исторические telemetry/sparse paths остаются включаемыми только явными флагами.

Проверки на Windows release: 31 profiler unit passed (6 ignored по умолчанию),
все 5 text runtime fixtures с настоящим текущим worker прошли явным запуском,
22 Python evaluator tests passed. Новый fixture проверил refine/cleanup/chain/
summary через настоящий прямой OpenAI-compatible HTTP endpoint, точные URI,
null trace, receipts, сохранность исходника/конфигурации endpoint и отсутствие
текста/имён в отчёте. Отдельный fixture с явным trace проверил прежнюю телеметрию;
retention/sparse guard expectations не ослаблялись. CI обновлён, удалённый runner
не запускался. Прежние предупреждения core не исправлялись в этом этапе.

## Native checks

Windows, NVIDIA RTX 5080 16303 MiB, driver 610.88, Ollama 0.40.14. По три
повтора неизменённых corpus у каждой модели: **576/576 expected**, в том числе
24/24 useful repairs в каждом из шести quality runs; missed/violation/review=0.
Все 12 коротких worker runs и четыре long runs завершились успешно. SHA corpus,
evaluator, prompts/guards, worker/profiler, source/manifests и .env/.env.defaults
совпали до/после; digests трёх установленных моделей также сохранились.

| Native ordinary cleanup | Wall, s | Сохранённых edits | Rejected / omitted |
|---|---:|---:|---:|
| Gemma4:e4b · 1 | 86.646 | 148 | 432 / 1 |
| Qwen3.5:4b · 1 | 164.040 | 126 | 413 / 3 |
| Gemma4:e4b · 2 | 85.287 | 145 | 433 / 0 |
| Qwen3.5:4b · 2 | 164.347 | 114 | 414 / 3 |

Rejected/omitted — счётчики worker, не число доказанных ошибок и не полнота
полезной правки. Текущие файлы весов: Gemma 6.58 GB, Qwen 4B 3.39 GB (decimal);
конкретные digests/метаданные в native-results.json. На этом desktop Gemma быстрее
примерно в 1.9 раза по wall; причинное улучшение от RV-020 и скорость Mac этим
экспериментом не установлены. Нет строгого cold/warm или эксклюзивного GPU режима.

## Targeted text review

Все 2816 line-visits прошли structural/source/receipt проверки. Детектор соседнего
overlap ≥8 слов не дал flags ни в одном long результате, но не доказывает всех
смысловых границ. Прежние опасные RV-020 замены Gemma 50/606 и Qwen 236/411/580
не повторились: эти строки у соответствующих моделей сохранены исходными.
Известный перенос соседнего текста в прежних RV-018/019 controls также не повторился;
некоторые другие модели на этих индексах сделали лишь поддержанные паузовые правки.

| Assessment | Fixed indices (194) | New evenly spaced (32) | Supplemental flags (284) |
|---|---:|---:|---:|
| Исходная строка сохранена | 133 | 0 | 0 |
| Поддержано текстовым контекстом | 59 | 30 | 267 |
| Спорное оформление | 2 | 1 | 13 |
| Требуется аудио | 0 | 1 | 4 |

Итого **510 row-visits**: 133 exact-source, 356 context_supported, 16 style_review,
5 audio_required. Из них 169 новых уникальных source/actual/context оценок,
208 явно помеченных повторных использований одинаковых четырёх hashes, 133 exact
source equality. Это агентский текстовый разбор, не независимая человеческая оценка
и не общая точность 704 строк.

Новые неоднозначные предметные замены — три source indices, пять row-visits:

- **240**, Gemma оба повтора: неизвестное слово заменено существующим предметным
  словом; соседний разговор о методе/загрузке не подтверждает новую сущность.
- **304**, Qwen первый и Gemma второй: неизвестная словоформа восстановлена в другой
  род, соседнее продолжение допускает иной референт. Текст не разрешает выбор.
- **649**, Qwen первый: незавершённое слово при самокоррекции дополнено до известного;
  контекст допускает разные продолжения. Во втором Qwen сохранён source.

Это **audio_required**, не доказанные ошибки распознавания/правки. Они подтверждают
практическую значимость уже записанного ограничения RV-020: отсутствие слова в
словаре не доказывает опечатку. Словарный fix существующее→существующее не выдаётся
за решение unknown→known. Прежние исправленные случаи не переоткрываются под теми
же ожиданиями; общее принятие модели остаётся открытым.

Style review: Gemma 6 (оба), 342 (первый), 541 (оба); Qwen 108/272/274 (оба),
175/310/385/463 (первый), 120 (второй). Слова сохранены, но сняты маркеры диалога,
потеряны/добавлены запятые либо точка разрывает незавершённое продолжение. Это
не оценка всех остальных пунктуационных edits как идеальных. Опасные замены не
обходились специальными списками слов; model/default не переключены.

## Reproduction and limits

[Подготовка и запуск corpus](../../../../docs/cleanup-quality.md),
[обычный profiler](../../../../docs/text-postprocessing-profile.md).
Новые quality/boundary manifests подготовлены прежним evaluator. Для каждой модели
и каждого corpus `--repeats 3 --timeout-sec 180`, затем тот же evaluator с
`--corpus ... --output ... --write .../quality.json`. Long manifest с одним
`cleanup` case ссылается на сохранённую публичную STT; четыре независимых запуска
`--repeats 1 --timeout-sec 900` в порядке Gemma/Qwen/Gemma/Qwen. Во всех командах
`--llm-base-url http://127.0.0.1:11434/v1`, per-run `--llm-model`, без trace/sparse.
Long outputs проверены `check-cleanup-quality.py --scan ...`.

До native по модели зафиксирован union прежних preselected/supplemental indices
и RV controls (50 Gemma, 47 Qwen). Дополнительно — 8 равномерных новых accepted
edits по заранее записанному правилу; lexical/critical/adjacent flags просмотрены
отдельно как supplemental, а не случайная выборка. Текстовый review читает source,
actual и обе соседние исходные строки. Повторный идентичный набор четырёх hashes
может использовать прежнее заключение с явным origin/reused_from. Exact-source
не оценивает качество самого ASR. Row-visits с повторами не равны независимым
gold примерам; из них не выводится corpus accuracy.

Audio/owner/Mac, native refine и качество summary в этом этапе **not-run**.
Короткий gate не проверяет все значения неизвестных слов и все нюансы пунктуации.
Файлы модели не являются замером RAM; настольные wall времена не являются
гарантией скорости на слабом Mac. Новая модель/default и автоматическая очистка
не приняты; точечные исключения под спорные слова не добавлялись.

## Integration validation

`cargo test --release --locked --offline -j 2 -p localvox-light-bench --bin localvox-profile`:
31 passed, 6 ignored. Затем с `LOCALVOX_PROFILE_TEST_WORKER` на текущий release
worker тот же Cargo test с фильтром `text::tests:: -- --ignored --test-threads=1`:
5 passed, 0 ignored. Python `unittest discover -s scripts/tests -p "test_*quality.py"`:
22 passed. Targeted rustfmt и `git diff --check` прошли.

OpenSpec 1.13.2: strict main **15 passed, 0 failed**, strict selected change valid
(`skip_specs`, только tooling/измерение); `check-openspec.py` — 15 capabilities,
88 requirements, 188 scenarios, 88 coverage rows, 1556 local links PASS.
UI/TUI пользовательское поведение не менялось; полный UI/audio suite повторно
не запускался, user-guide не расширялся инженерными флагами. Рабочий daemon и
пользовательский архив не затронуты.
