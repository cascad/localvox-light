# План проверок требований

Дата планирования: 2026-10-31. Все требования ниже — **proposed delta**; реализация в работе. Таблица сохраняет план проверок, фактические команды, результаты и пределы записаны в [implementation](implementation.md). Полный паритет и аппаратная приёмка не пройдены.

Это change-матрица, не расширение принятого baseline. Новые ID не переносятся в `openspec/evidence/coverage.md` до соответствующего принятия/sync. Затронутые существующие требования обновляются в main evidence по фактическим проверкам реализации; исторические результаты не переименовываются.

| Требование | Планируемые точки реализации | Проверка / задачи | Текущий результат |
|---|---|---|---|
| [JRN-001](../specs/technical-journal/spec.md#requirement-jrn-001-дерево-операций-в-обоих-клиентах) | core progress, API journal, React Journal, новый terminal renderer | 2.1, 3.2, 5.3; одинаковая иерархия fixtures и различимые стили | [частичный test-run](implementation.md); полная приёмка not-run |
| [JRN-002](../specs/technical-journal/spec.md#requirement-jrn-002-достоверные-состояния-и-время) | workflow/оба клиента | 2.1, 3.3, 5.3; failure с готовым текстом, heartbeat, unknown time | [частичный test-run](implementation.md); полная приёмка not-run |
| [JRN-003](../specs/technical-journal/spec.md#requirement-jrn-003-сворачивание-и-чтение-журнала) | state/render UI/TUI | 3.2, 5.3; ровно 35 строк, focus/scroll, новые события | [частичный test-run](implementation.md); полная приёмка not-run |
| [JRN-004](../specs/technical-journal/spec.md#requirement-jrn-004-доступный-по-api-ограниченный-журнал) | core bounded reader, api HTTP | 2.2, 2.5; cursor/reset/partial/limits/auth/legacy | [частичный test-run](implementation.md); полная приёмка not-run |
| [JRN-005](../specs/technical-journal/spec.md#requirement-jrn-005-безопасные-команды-и-логи) | producers/API/display sanitizer | 2.3, 2.4; secrets/OSC/HTML/oversize/spool fault | [частичный test-run](implementation.md); полная приёмка not-run |
| [JRN-006](../specs/technical-journal/spec.md#requirement-jrn-006-доступная-компоновка) | UI tokens/layout, terminal theme/layout | 3.1, 5.3; narrow/resize/ASCII/monochrome/focus/reduced-motion | [частичный test-run](implementation.md); полная приёмка not-run |
| [TERM-001](../specs/terminal-workspace/spec.md#requirement-term-001-один-демон-и-самостоятельный-клиент) | новые client/terminal, launcher | 1.1, 9.1, 9.3, 10.2; no core deps, два клиента, quit при записи | [частичный test-run](implementation.md); полная приёмка not-run |
| [TERM-002](../specs/terminal-workspace/spec.md#requirement-term-002-архив-и-документы-с-паритетом-действий) | terminal archive/documents/actions | 4.1–4.3; P01–P08/P20–P22 с payload/error/confirmation | [частичный test-run](implementation.md); полная приёмка not-run |
| [TERM-003](../specs/terminal-workspace/spec.md#requirement-term-003-запись-импорт-и-очередь) | terminal recorder/queue/ingest | 5.1–5.2; P09–P11/P18–P19, accepted≠confirmed | [частичный test-run](implementation.md); полная приёмка not-run |
| [TERM-004](../specs/terminal-workspace/spec.md#requirement-term-004-поиск-чат-заметки-и-вопросы) | terminal search/chat/notes/asks | 6.1–6.3; P12–P17/P23–P24, explicit provider/lost-ack | [частичный test-run](implementation.md); полная приёмка not-run |
| [TERM-005](../specs/terminal-workspace/spec.md#requirement-term-005-плеер-внутри-терминального-приложения) | terminal playback, process owner, media API | 8.1–8.3, 10.2; P25–P27, real IPC/Range и отдельно физический звук | [частичный test-run](implementation.md); полная приёмка not-run |
| [TERM-006](../specs/terminal-workspace/spec.md#requirement-term-006-настройки-и-авторизация) | terminal settings/connectivity | 7.1–7.2; P28–P30, dirty/unset/secrets/restart/auth | [частичный test-run](implementation.md); полная приёмка not-run |
| [TERM-007](../specs/terminal-workspace/spec.md#requirement-term-007-управление-клавиатурой-и-формы) | terminal update/forms/runtime guard, именованные Action/COMMANDS | 1.4 и каждый срез; Unicode/paste/PTY/resize/raw cleanup; q/F10 выход и y копирование, формы | [частичный test-run](implementation.md), [q/F10 и цельный текст](../../control-postprocessing-and-terminal-player/evidence/plain-text-and-terminal-exit.md), [рефакторинг 2026-11-01](../../refactor-terminal-command-dispatch/evidence/implementation.md): палитра/контекст, подтверждения, ConPTY F8/F9 из формы; конкретное окно VS Code/физическая приёмка not-run |
| [TERM-008](../specs/terminal-workspace/spec.md#requirement-term-008-отзывчивость-и-актуальность-данных) | client I/O, app generation/cache | 1.3, 2.5; stale response/backpressure/timeout/reconnect | [частичный test-run](implementation.md); полная приёмка not-run |
| [TERM-009](../specs/terminal-workspace/spec.md#requirement-term-009-консоль-ошибок-и-подтверждение-просмотра) | diagnostics, app contexts, console renderer | 11.5/11.7; unread/ack/recovery, +N repeats, phase/session, bounds, safe log, ConPTY | [test-run 2026-11-01](usability-2026-11-01.md); история текущего TUI, физическая приёмка not-run |
| [RUN-002 delta](../specs/runtime-settings/spec.md#requirement-run-002-комплект-бинарников-и-ui) | workspace/run/install/release | 9.2; binary комплект, BuildOnly/build failure | [test-run](implementation.md): dev build, isolated script cases and copied binary smoke; release CI not-run |
| [RUN-004](../specs/runtime-settings/spec.md#requirement-run-004-новый-терминальный-запуск) | launcher/main/CLI | 9.1–9.3; API identity/15s/remote/no duplicate engine | [test-run](implementation.md): launcher process + ConPTY actual main→client, client quit sends no mutations; real capture not-run |

## Проверки планового пакета

Выполнены 2026-10-31 на Windows из корня репозитория, официальный CLI 1.13.2:

| Команда | Фактический результат | Граница |
|---|---|---|
| `npx --yes @fission-ai/openspec@1.13.2 validate --specs --strict --no-interactive` | PASS: 15 passed, 0 failed; информационные замечания о длине существующих требований | Форма принятого baseline, не реализация |
| `npx --yes @fission-ai/openspec@1.13.2 validate add-technical-journal-workspace --type change --strict --no-interactive` | PASS: change is valid | Форма proposal/delta/tasks |
| `python scripts/check-openspec.py` | PASS: 15 capabilities, 86 requirements, 168 scenarios, 86 coverage rows; локальные ссылки проверены | Структурная трассируемость, не тесты продукта |
| `git diff --check` и `git -c core.autocrlf=false diff --no-index --check -- NUL <file>` для каждого нового файла change | PASS; whitespace-замечаний нет | Во втором вызове exit 1 без диагностик означает наличие diff, а не ошибку whitespace |

На момент завершения планирования `status --change add-technical-journal-workspace --json` показывал все четыре плановых артефакта `done`, `isPlanningComplete=true`. Поле CLI `isComplete=true` здесь относится к комплекту документов: тогда было **0/34 задач реализации выполнено**. Текущий прогресс определяется [tasks](../tasks.md).

При планировании новые UI/TUI, API событий, harness и playback backend не реализовывались. Последующие прогоны приведены в implementation.md. Реальный архив и работающий демон не изменялись. Исходный `engineering-doctrine.md` уже был untracked до планирования и не менялся.

## Замечания владельца — 2026-11-01

TERM-002/003/005/006/007/008/009 и JRN-003/005 дополнительно проверены в [новом отчёте](usability-2026-11-01.md): metadata/copy, импорт, настройки, constrained seek, waveform retry, console/repeat/ack, HTTP summary и UI/TUI. TERM-005: исходный сбой слышимого звука остаётся не воспроизведён, 11.3 и аппаратная 10.2 открыты.
