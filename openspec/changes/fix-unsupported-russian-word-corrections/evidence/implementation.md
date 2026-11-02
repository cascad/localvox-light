# RV-020: основание русской поправки — 2026-11-02

Воспроизведённые неподтверждённые словоформы отклоняются новым общим guard.
Модели/default/provider/.env не переключались, Ollama и новые native LLM-вызовы
не использовались. Это исправление admission, не новая приёмка моделей.

## RED → GREEN

База `59e7640`. Новый тест
`correction_guard::tests::existing_russian_words_require_evidence_beyond_spelling_distance`
на прежнем guard: exit 101, 1 failed, 105 filtered; первый отказ — необоснованная
замена падежа после предлога. Исходные corpus и допустимые ответы не изменялись.

Добавлены собственные регрессии для падежа, числа/времени, предметного слова,
неизвестного слова, неоднозначного подлежащего/дополнения/обращения и обхода через
двухсловный ASR repair. Полезные typo/согласование, включая местоимения и новое
предложение, проверяются отдельными позитивными controls.

## Реализация и границы

[Общий guard](../../../../crates/localvox-light-llm/src/correction_guard.rs) вызывает
[русское словарное основание](../../../../crates/localvox-light-llm/src/correction_guard/russian.rs).
`rs-morphy4 = 0.1.3`, exact pin + Cargo.lock; добавлена только одна новая зависимость.
Используется неизменённый OpenCorpora словарь, без prediction/частотного ranking.
Словарь embedded, lazy OnceLock, runtime файлов/сети и дополнительного процесса нет.
Ошибка словаря отклоняет сомнительную замену; этот путь code-reviewed, искусственное
повреждение встроенных таблиц в этом прогоне не моделировалось.

Для двух существующих русских слов буквенная близость сама по себе недостаточна.
Неизвестное→известное допускается в прежнем edit budget, неизвестное→неизвестное
сохраняет source. Отдельное узкое правило допускает исправление согласования:
неизменённое непосредственное подлежащее в начале предложения, все его разборы
именительные, тот же глагол прошедшего времени с прежними прочими признаками.
Старая форма не согласуется ни с одним разбором подлежащего, новая — со всеми.
Иначе сохраняется источник. Не выбираем наиболее вероятный parse вместо проверки
неоднозначности. Group repair не обходит словарь и ограничен длиной слова 128.

Регистр/пунктуация/паузы/повторы/пробелы и канон глоссария сохранили прежние пути.
Нерусские буквенные замены остаются под прежними guards. Ограничение неизвестного
слова — существенное: отсутствие в словаре не доказывает опечатку; новый термин
или имя могут отсутствовать. Полный смысл не доказывается; grounding/литералы/числа
не заменены этим словарём. Сомнительная строка сохраняется целиком, без нового
частичного переписывания и без миграции старых результатов.

[Attribution и лицензии](../../../../licenses/rs-morphy4/README.md) включены в release
archive workflow. Пять upstream notice/license текстов побайтово сверены с crate
и копией тестового пакета. Данные словаря в Git не добавлялись. Полный release
archive/installer и CI runner не запускались; проверены реальные файлы и их копия,
workflow содержит проверки наличия notice/CC license перед упаковкой.

## Выполненные unit и runtime-mock

| Проверка | Результат |
|---|---|
| `cargo test --locked --offline -j 2 -p localvox-light-llm --lib -- --test-threads=2` | 106 passed |
| `cargo test --locked --offline -j 2 -p localvox-light-bench --bin localvox-profile` | 31 passed, 5 ignored |
| explicit ignored `text::tests::` с текущим debug worker | 4 passed |
| сборка debug worker / release correction checker | PASS |
| targeted rustfmt guard/russian/worker fixture | PASS |

Расширенный настоящий worker fixture запускает refine и cleanup через fake loopback
provider на 16 собственных строках. Пять полезных правок приняты, две новые опасные
словоформы и прежние отказы сохранены исходными; cleanup rejected=10, omitted=0.
Проверены все строки (включая количество), первичные bytes, parent=[1],
source_id/start/end/speaker и валидные receipts. Refine rejected counter отдельно
не asserted; проверяется результат и метаданные. Это runtime-mock, не native LLM.

Первый offline запуск после добавления dependency не разрешил локальный индекс
rustix; обновление индекса штатным Cargo добавило только rs-morphy4, без обновления
остальных locked packages. Последующие `--locked --offline` прошли. Инструментарий:
rustc 1.96.1, Windows; прежние предупреждения core не исправлялись.

## Replay, без запуска моделей

[Решения и hashes](replay-results.json): 18 групп, 4800 source/candidate решений.
Перед replay существующий evaluator проверил source bytes/version/receipts/edits
каждого профиля. Это сохранённые выходы прежних worker, **не новые native calls**.

- Старые 72-case Gemma/Qwen по три повтора: после guard 72/72 expected и
  **24/24 useful** в каждом. Ожидания/corpus не подгонялись под фильтр.
- По три 24-case boundary каждого: 24/24 expected сохранены.
- Четыре длинных результата из предыдущей release серии:

| Сохранённый результат | До | Прошло новый guard | Pure guard на 704 строках, ms |
|---|---:|---:|---:|
| Gemma · 1 | 172 edits | 156 | 10.080 |
| Qwen · 1 | 138 edits | 119 | 6.952 |
| Gemma · 2 | 154 edits | 140 | 8.004 |
| Qwen · 2 | 144 edits | 125 | 7.821 |

RV-020: Gemma 50 в первом и 606 в обоих повторах отклонены. Все 5 ранее
audio_required Qwen замен (236/411 в обоих, 580 в первом) также отклонены;
это не превращает их прежнюю неопределённость в доказанную аудио-ошибку.
Из 61 ранее context_supported row-visits сохранены 46, для 15 оставлен source.
Все 3 meaning_risk row-visits теперь source; 2 style_review с тире остаются.
Прежние 83 exact-source row-visits не изменились. Эти повторные визиты целевой
agent-review выборки не являются новым независимым gold или общей точностью.

Ещё два старых полных результата до retention fix: Gemma 143/559 и Qwen 120/509
edits прошли. Прежние опасные 439 и 95/256/267/666/686/702 отвергнуты. Принятие
меньшего числа edits — цена консервативности, не само по себе улучшение качества.

## Цена словаря

[Пять пар измерений before/after](cost.json), release checker на Windows, одинаковые
runtime DLL рядом с обоими exe. Каждый новый процесс: два одинаковых typo checks,
затем 704 пары первого Gemma. Файловый кеш не очищался. Первый check включает
одноразовые guard/lexicon/dictionary действия, не изолированную загрузку DAWG.

| Медиана / размер | До | После |
|---|---:|---:|
| Первый word check, ms | 0.376 | 8.508 |
| Следующий такой же check, µs | 6.8 | 22.1 |
| Следующие 704 пары, pure guard, ms | 9.333 | 9.680 |
| Peak RSS checker, MiB (диапазон пяти процессов) | 6.98–7.40 | 27.06–27.31 |
| Размер checker, MiB | 1.054 | 9.638 |

Peak получен `GetProcessMemoryInfo` по сохранённому CreateProcess handle после
завершения, CPU — `GetProcessTimes`; raw значения/availability в JSON. Это весь
checker process, **не память модели/worker/Ollama**, не Mac/MPS и не cold disk.
Проверка остаётся малой относительно генерации; ускорения LLM эта работа не делает.

## Воспроизведение и not-run

[Replay CLI](../../../../docs/correction-guard.md): release checker, повторяемые
`--profile` старых quality/boundary, `guarded-long-*` и `sparse-long-*-baseline-b`,
новый output. Точные report/checker/source/dependency SHA — в JSON.
Полные source/actual остаются в target на D:, в Git только hashes/индексы/заключения.

Новые native модели/прогоны, полная аудио/owner приёмка, Mac/CPU-only/MPS,
summary/refine native качество, SAGE/ONNX и обновление рабочей установки **not-run**.
RV-020 исправлен в воспроизведённом объёме, универсальная смысловая безопасность
не заявлена. Рабочий release worker этим этапом не пересобирался: собраны debug
fixture worker и отдельный release engineering checker.

## Итоговая проверка

- Python evaluator tests: 22 passed.
- `openspec validate --specs --strict --no-interactive`: 15 passed, 0 failed;
  информационные сообщения о длине требований сохранены отдельно от результата.
- Strict validation `fix-unsupported-russian-word-corrections`: valid.
- `python scripts/check-openspec.py`: PASS — 15 capabilities, 88 требований,
  188 сценариев, 88 coverage rows, 1490 локальных ссылок.
- Проверенные delta добавили 3 сценария LLM-003/PIPE-006, прежние сценарии сохранены.
- `git diff --check` и итоговый staged diff: PASS. Source/checker/worker hashes
  сопоставлены с JSON evidence; corpus, provider/settings и старые outputs не менялись.
