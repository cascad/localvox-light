# Cursor-страницы и баланс времени — 2026-11-21

PIPE-011, US-007. Это продолжение проверки, а не переписывание прежнего результата.

## Дефект и причина пропуска

В новой URL-записи stage overview знал длительности, но journal на следующих
опросах показывал «исторический · запуск неизвестен» и «время неизвестно».
StageEvent сохранялся без run; reader держал RunMark только в пределах одного
запроса. Начало и завершение попадали в разные клиентские группы.
Прежние client/render tests получали уже нормализованный run и этот путь не проходили.

Регрессия `stage_identity_survives_successive_cursor_reads_and_legacy_pages`
до исправления упала: на следующей странице ожидался `old`, вернулся пустой run.
Теперь stage records самодостаточны; строгий standalone marker не путается с
record, содержащим run. Старый формат получает контекст только из реального
предшествующего маркера в оставшемся бюджете 1 MiB. Текущая metadata и запоздалая
операция другого запуска не подставляются в историю. Пустой poll не сканирует её.
Отдельный тест проходит с маркером за пределами бюджета для новых records.

## Проверенная цепочка

- `producer_cursor_pages_preserve_stage_clocks_in_the_terminal_client`: настоящие
  producer и HTTP; страницы по одному событию, heartbeat/child/end/completion,
  настоящий decoder и группировка TUI. Один этап остаётся одной группой с его часами.
- `shared_workspace`: настоящий embedded UI и TUI в ConPTY против одного HTTP и
  временного архива; synthetic producer явно создаёт результаты без inference.
  Браузер проверяет баланс, отсутствие неизвестных часов и запусков, раскрытие
  журнала, затем заметки, аудио, вопросы, настройки и reconnect в обоих клиентах.
  Стенд исправлен под уже принятую начальную фокусировку меню и сохранение view
  при reconnect; отсутствующий refined результат больше не объявляется Done.
- Accounting unit tests: heartbeat, завершение, failed→retry, пересечения,
  паузы, live clock, неизвестные/обратные/невалидные границы.
- UI/TUI проверяют реально отображённый баланс и округление, включая перенос 59,96 с
  и случай, когда независимое округление дало бы 17,4 + 1,2 = 18,7.

## Исходная запись: read-only

Проверен release standalone API на отдельном loopback-порту без capture engine,
очереди исполнителей, повторного STT/LLM и изменения progress.jsonl (SHA-256 совпал).
71 последовательный cursor-запрос по одному событию: 71 событие, 0 неизвестных run,
один корректный Done для каждого выполненного этапа. Бюджет чтения соблюдён.

Измеренные интервалы этапов: download 10,789484 с; extract 0,109450 с;
transcribe 1,124460 с; summary 5,421141 с. Расчёт календарного баланса использует
millisecond clocks API: этапы 17,446 с + запуск/ожидание/переходы 1,225 с =
18,671 с после старта; первая очередь 2,804 с; всего 21,475 с. Пересечений нет.
Показ: 17,4 + 1,3 = 18,7; 2,8 + 18,7 = 21,5 с. Исходные времена не подгоняются.

## Выполненные проверки

- Rust libraries: core **451 passed / 9 ignored**, API **94 passed / 8 ignored**,
  terminal **218 passed / 3 ignored**. Игнорируемые hardware/model/helper сценарии
  этим прогоном не объявляются проверенными.
- Все **9 ConPTY tests passed**; отдельный `shared_workspace` **passed**.
- UI **16 unit tests**, **82 browser e2e passed**, production build passed.
  Три отрицательных visual-detector проверки ожидаемо обнаружили искажения.
  Эталоны скриншотов не обновлялись.

## Локальная поставка

Release build daemon + worker + terminal clients + standalone API с ONNX
через feature unification ASR crate прошёл. После проверки отсутствия запущенных
установленных процессов в `F:\lv` заменены только `localvox-light.exe`,
`localvox-process.exe`, `localvox-tui.exe`, `vox.exe` и обновлён bundle manifest.
Старые файлы сохранены в ignored `target/review/journal-stage-2026-11-21/original`.
SHA-256 `.env` и исходного progress.jsonl совпали; модели/архив не изменялись.
Manifest содержит актуальные hash/bytes и `release_tag: null` для локальной сборки.
Installed daemon help, worker protocol 3, версии обоих клиентов и bundle-check
прошли через native diagnostic guard без DLL-диалога.

Проверка новых моделей, устройств и macOS этим изменением не выполнялась.

Strict checks прошли: main specs 15/15, выбранный change, check-openspec
(15 capabilities / 88 requirements / 188 scenarios / 88 coverage rows), diff-check
и rustfmt изменённых Rust-файлов. Отдельный journal_http_contract passed.
UI lint завершился без ошибок с 9 существующими предупреждениями в неизменённых
компонентах. Новые регрессии входят в существующий Interface workflow:
journal/workflow/workspace, browser e2e и явно запускаемый shared_workspace.
