# Пропуск нерелевантных role окон

2026-11-09. Оптимизация NER сохраняет полный поиск имён и выполняет поиск ролей
только в полных прежних окнах, пересекающих найденные имена. Размер/контекст окон,
модель, thresholds и decoder не менялись. [Численные данные](native-results.json).

## Обоснование и реализация

Каждый role span ограничен словами своего окна. Любой role span, способный
пересечь name, находится в окне, которое тоже пересекает name. Такие окна
выполняются целиком: сохраняются model input, контекст и локальный NMS, включая
влияние соседних spans. Пропущенное окно не может дать участвующий в сравнении
role span. Глобальный dedup и сравнение scores остаются прежними.

[Общий collector/индекс](../../../../crates/localvox-light-core/src/ner/windows.rs)
строит прежние окна из одного iterator. Индекс объединяет отсортированные
half-open byte intervals и использует binary search; не опирается на порядок,
язык, labels или confidence имён. Набор names не меняется при построении индекса.
Пустой набор имён не требует role inference, плотный может не дать выигрыша.
Окна не обрезаются вокруг имени, не объединяются и не сдвигаются.

[Ner.names](../../../../crates/localvox-light-core/src/ner.rs) использует relevant
scan; общий generic extract_at остаётся полным. [Profile](../../../../crates/localvox-light-core/src/ner/profile.rs)
считает skipped отдельно от started/completed/inference. Explicit full-scan
reference использует те же inference/decoder/dedup/comparison и доступен только
как инженерный API; автоматического fallback нет. Обычный путь без clocks/логов.
Ошибки необходимых вызовов сохраняются; заведомо ненужные вызовы не запускаются.

Нет списков слов, приблизительных порогов, новой модели, threads tuning или
постоянного кеша. Это исключение вычислений по строгому условию влияния на
результат, а не ослабление проверки. Правило владельца об устойчивости и
предварительном обсуждении необходимых хрупких решений сохранено в AGENTS.md.

## Model-free и synthetic native проверки

- Core `cargo test --locked --offline -j 2 -p localvox-light-core --features onnx ner:: --lib`
  — **9 passed, 2 ignored**. Пять новых cases проверяют оконные границы без лишнего
  хвоста, Unicode/union/nested/unsorted/adjacent intervals, 27600 сравнений индекса
  с определением пересечения, 150 полных/сокращённых role decisions с оконными
  дублями, пустой/плотный набор names, неизменность всех retained input words и
  propagation ошибки необходимого окна с partial/counters. Остальные проверяют
  прежнее разбиение слов и выключенный observer/privacy/error/finite timing.
- Bench profiler — **6 passed, 1 ignored**; LLM — **108 passed**. Изменений provider/
  correction guard/очереди/клиентских действий нет. UI/TUI/e2e заново не запускались.
- Release native synthetic — **1 passed, 8 случаев / 26 entities**: ordinary,
  profiled и full-scan результаты совпали полностью, включая f32 scores, порядок
  и byte spans. Есть overlap, пустой текст/role label и thresholds 0.3/0.5.
- Release probe build и targeted rustfmt — passed; модель и рабочий worker не
  пересобирались/не заменялись. Команды/границы — [в engineering guide](../../../../docs/summary-grounding-profile.md).
- Strict main specs — 15 passed, 0 failed; strict выбранного change — valid,
  zero deltas accepted при skip_specs. `python scripts/check-openspec.py` — PASS:
  15 capabilities, 88 requirements, 188 scenarios, 88 coverage rows, 1530 links.
  `git diff --check` — passed; model-free команды и rustfmt добавлены в CI.

## Длинное парное сравнение

Два fresh release native test процесса на прежней публичной Podlodka #109:
7146.527375 s записи, 704 реплики, 104622 chars joined speech. Source JSONL SHA
`09014305b713638301d3cd4693d03e1f7f2920e75bdfc749c7164d0e10034ae8`.
Это прямой source NER на joined speech, не полная генерация сводки.
Baseline исходников `4e3249d` плюс этот срез; SHA шести source files, probe и native
test binary зафиксированы до/после. Сборки завершены до серии; Cargo запускает
готовый test executable со штатным runtime environment, без перекомпиляции.

| Порядок | Full NER, s | Relevant NER, s | Уменьшение времени | Full role окна | Relevant / skipped | Entity equality |
|---|---:|---:|---:|---:|---:|---|
| Full → relevant | 47.118435 | 36.758561 | 21.99% | 214 | 125 / 89 | 151/151, exact |
| Relevant → full | 50.637353 | 38.484049 | 24.00% | 214 | 125 / 89 | 151/151, exact |

Name pass остаётся полным: 214 окон. Всего inference 428 → 339 (−89), skipped
не выдаются за выполненные. Оба сравнения подтвердили точные Entity/score/spans,
не только одинаковое число сущностей. Threshold source 0.3, name/role labels
обычные, role pass 0.1. Все фактически нужные вызовы завершились без ошибок.

Windows AMD64, Ryzen 9 9900X3D / 6 logical processors, тот же default CPU ORT
`2.0.0-rc.12`/Level3, без новых GPU providers. GLiNER `model_fp16.onnx` SHA
`9cec3c771bfa7e886e029cd013326b13ad7d18f58fac33824f43c106fbdfe920`;
tokenizer/config/остальные model hashes сохранены в JSON. Файловый кеш не очищен,
hashes читают модели до запуска, desktop-нагрузка не контролировалась.

Первый прямой запуск test exe не дошёл до test entry без Cargo runtime environment;
остановлен только собственный процесс, данных о производительности не дал.
При сборе успешной первой пары harness ошибочно ожидал 221 окно для сырого speech.
Native Entity test уже прошёл; harness исправлен на сравнение с full plan того же
входа, готовый отчёт принят только после сверки SHA. Исходники/тестовые ожидания
Entity под результат не менялись. Следующая пара выполнялась в обратном порядке.

## Обычный grounding и пределы вывода

Отдельные fresh release profiler процессы проверяют прежнюю STT/готовый summary
markdown (SHA `3b44981a9c17877775fbba7ffb81661addc5d93633c4d8e4faf95d4ed520f778`),
с built-in template/lexicon, пустым glossary и без LLM/STT/download. Grounding
применяет прежний `plain`, соединяя непустые строки через `. `; поэтому его
source содержит 22088 words / 221 окно, а прямой joined speech — 21386 / 214.
Их wall нельзя считать одним и тем же workload. Preprocessing не менялся.

| Обычный профиль | R1, s | R2, s |
|---|---:|---:|
| Source NER | 38.294486 | 40.589414 |
| Первый grounding check (source+answer+остальная проверка) | 40.494716 | 43.165696 |
| Повторный check с кешем source | 2.205169 | 2.474739 |
| Ordinary unprofiled parity check | 2.958874 | 2.464373 |
| Model load | 1.015262 | 1.082861 |
| Подготовка/фингерпринты инструмента | 0.531728 | 0.557041 |
| Полный wall инженерного процесса | 47.799784 | 50.288490 |

В обоих профилях name pass 221 окон, role 138 выполнено / 83 skipped;
source inference 442 → 359 при прежнем нормализованном входе. Backend сделал
1 source + 3 answer requests; повторный check и ordinary не повторяли source.
Ordinary core profiles пусты, exact verdict parity выполнена. Во всех checks
остались 1 name / 2 number flags: исходная summary остаётся unverified.
Это сохранение результата проверки, не приёмка качества её содержания.

Все четыре процесса завершены штатно в пределах 300 s. SHA source/answer/model
до/после совпали; frozen code/probe/native test/рабочий worker SHA сохранены.
Никакого текста/имён/entities/spans в numeric evidence нет, только counts/SHA/times.
Вложенные core components не прибавляются повторно к backend/check wall.

Полная generation, качество новой summary, Mac/слабое железо и аппаратная приёмка
не проверены; универсального SLA нет. На плотных именах ускорение может отсутствовать.
Модели/настройки/defaults/`.env` и архив владельца не менялись. Рабочий daemon/worker
не обновлялся: прежний worker SHA
`85474d4ed43753f3f1bd51217e104825cbdf19991a5fb0b13eaa62886a6e1696` сохранён.
Нового пользовательского действия нет; справочник не перегружен engineering flags.
