# Последовательное чтение документов · 2026-11-01

Продолжение работ владельца по медленной обработке и очистке интерфейсов.
Работающий демон, пользовательский архив и .env не менялись.

## Исходный отказ

В [старом hook внутри DocumentPane](../../../../ui/src/DocumentPane.tsx) новый tick
увеличивал seq и начинал GET независимо от уже активного запроса. При задержке
ответа дольше интервала предыдущий полезный ответ отбрасывался. Cleanup не было;
закрытые вкладки продолжали ждать сеть, React StrictMode мог начать дополнительное
чтение при проверочном mount.

Браузерный тест `slow document refresh is serialized and coalesced without hiding
the first usable answer` удерживает ответ processed, продвигает часы на 7100 ms
и требует одно чтение. До исправления: FAIL, expected 1 / received 4. Лог локально
в `target/artifact-refresh-before.log`. Это воспроизведение клиента на mock HTTP,
не доказанная причина задержки GigaAM/LLM и не benchmark двухчасовой записи.

## Реализация и сценарии ARCH-008

| Сценарий | Код / проверка | Результат |
|---|---|---|
| Медленный API | [useArtifact](../../../../ui/src/useArtifact.ts), [browser tests](../../../../ui/tests/e2e/workspace.spec.ts) | Один GET во время нескольких refresh; первый полезный ответ виден, один накопленный повтор идёт последовательно. Во время повтора текст остаётся, «загрузка…» не возвращается. |
| Финальное обновление | Тест `the final document refresh survives processing finishing during a slow read` | Live → false при удерживаемом GET сохраняет финальный refresh; после освобождения виден итоговый документ, дальнейших периодических GET нет. |
| Уход с вкладки | Четыре параметризованных теста `leaving ... cancels every pending artifact GET` | Отмена наблюдается на реальном signal fetch для summary, processed, transcript, versions и speakers; поздние ответы не показываются. |
| Смена сессии/версии | Дополненный `archive keeps its last rows ...` и новый `changing the transcript version ...` | Прежний GET отменяется; актуальная другая сессия/выбранная версия сохраняется после освобождения старого ответа. |
| Ошибка/восстановление | `document errors recover on the next refresh without switching tabs` | 503 показан, следующий успешный refresh возвращает документ без переключения вкладки. |

Вызовы в [DocumentPane](../../../../ui/src/DocumentPane.tsx) используют отдельный hook;
[api.ts](../../../../ui/src/api.ts) добавляет только необязательный AbortSignal к
пяти GET. URL, payload и ответы не менялись, 15-секундный timeout сохранён.
Lifecycle определяется key сессии/версии; snapshot также помечен key. Отмена
только завершает ожидание клиента, не заявляет остановку backend.

## Выполненные команды

OpenSpec CLI 1.13.2; локальные логи `target/artifact-refresh-*` не версионируются.

| Проверка | Результат |
|---|---|
| Целевой browser прогон новых сценариев | 8 passed |
| `npm --prefix ui test` | 9 passed |
| `npm --prefix ui run test:e2e` | 42 passed; 8 новых и прежние 34, включая 3 ожидаемых отрицательных проверки визуального детектора |
| `npm --prefix ui run build` | PASS: TypeScript/Vite |
| `npm --prefix ui run lint` | PASS: 8 прежних warnings (6 Fast Refresh, 2 ref cleanup), ошибок нет |
| `cargo test --locked -j 2 -p localvox-light-api --lib -- --ignored --exact http::tests::cross_clients::shared_workspace --nocapture` | 1 passed, 4.60 s: настоящий временный API/архив, встроенный свежий UI в Chromium и TUI в ConPTY |
| Строгая валидация main specs/change | PASS: 15 main specs, fix-browser-artifact-refresh; информационные замечания о длине текста требований |
| `python scripts/check-openspec.py` | PASS: 15 capabilities, 86 main требований, 168 сценариев, 86 строк coverage |
| `git diff --check` и новые текстовые файлы | PASS; новых/изменённых screenshot baseline нет |

## Границы

- Снимки UI не менялись и не принимались автоматически; полный прогон выполнен
  с существующими baseline. Rust TUI не менялся, его 95 прежних тестов не выдаются
  за новый отдельный прогон.
- Проверки охватывают чтение артефактов. Общий shell poll, mutations, запись,
  плеер и модельная обработка этим hook не управляются.
- Клиентская отмена не гарантирует прекращение уже начатой работы HTTP-сервера.
  Численная производительность ASR/LLM и BL-003 здесь не проверялись.
- Физические устройства, системный clipboard, реальная двухчасовая запись
  не проверялись; 10.2 add-technical-journal-workspace остаётся not-run.
- ARCH-008 пока находится в delta; main baseline не синхронизировался, changes
  не архивировались. Коммиты выполняет пользователь, агент индекс не менял.
