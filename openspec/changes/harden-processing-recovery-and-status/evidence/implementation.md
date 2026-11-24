# Проверка исправлений, 2026-11-23

## Основание и граница

Владелец показал Mac: длительный импорт без результата, повторяющийся отказ поиска
ASR-модели, смешанную историю запусков, скрытые команды палитры и пустую «Сводку»
после summary skipped. Уточнил: распознанный текст есть, одна короткая строка.
Затем выбрал одну полную переварку по r, с сохранённого аудио, вместо ручного
обновления и разных видов повтора. Это пользовательские наблюдения; исходная
Mac-сессия не копировалась и на Windows не запускалась.

## Что исправлено и чем проверено

| Наблюдение / требование | Реализация и регресс |
|---|---|
| Импорт долго остаётся в download | [download.rs](../../../../crates/localvox-light-ingest/src/download.rs): socket timeout 10 с, внутренние retries yt-dlp отключены, недоступный фрагмент — отказ, общий process supervisor ограничивает запуск 30 минутами. Необязательное название ограничено 10 с. Координатор владеет серией из трёх попыток. Исполняемый fixture проверяет аргументы, успешный итоговый путь, stderr отказа, таймаут и удаление своих частичных файлов. Реальный YouTube/network throughput не измерялся. |
| Отсутствующая модель повторяется бесконечно | [jobs.rs](../../../../crates/localvox-light-core/src/jobs.rs), [prefix.rs](../../../../crates/localvox-light-core/src/jobs/prefix.rs): три запуска фазы, включая прерванные. Failed не возрождается maintenance, загрузкой очереди, новым horizon или Stop. `three_failed_attempts_stop_across_restarts_until_manual_retry`, `interrupted_attempts_cannot_buy_an_unbounded_restart_budget` и prefix tests проверяют сохранение состояния и ручной новый бюджет. |
| После рестарта нужен понятный ручной запуск | [HTTP workspace](../../../../crates/localvox-light-api/src/http/tests/workspace.rs): `a_stopped_job_survives_client_restart_and_full_recook_keeps_its_audio` проверяет повторные подключения без записи в jobs.json, затем scope=all → Pending/Prepare/attempts=0/force, сохранность WAV и прежних документов. `forced_link_recook_keeps_audio_proof_and_only_future_steps_through_http` проверяет переиспользование аудио ссылки. Полный recook снимает старый incremental intent (`explicit_full_recook_retires_a_failed_early_intent_and_starts_from_audio`). |
| Старые ошибки смешаны с новым успехом | История не удаляется и не служит разрешением автоматически переработать запись. Готовность по-прежнему определяется проверенными receipts. Для первого раннего Transcribe, который обходит Prepare, [координатор](../../../../crates/localvox-light/src/autocook.rs) теперь сохраняет run ID до исполнения. Тест `an_early_asr_failure_before_worker_start_has_one_saved_run_and_a_measured_stage` проверяет одно nonempty run для running/failed и измеримые начало/конец при отказе до запуска worker. Старым событиям неизвестные границы не выдумываются. |
| Сводка «завершена», но файла нет | [processing.rs](../../../../crates/localvox-light-core/src/processing.rs) сохраняет различие «нет распознанной речи» и «модель не создала содержательной сводки по тексту». Старые две причины читаются с понятными подписями, без миграции файлов и автоматической инвалидации. Summary API возвращает available/status/reason; pipeline сообщает «завершена; сводка пропущена» с причиной. `skipped_summary_keeps_the_short_transcript_and_explains_why_there_is_no_file` проверяет одну непустую реплику, legacy Nothing receipt, повторные чтения без enqueue и отдельную причину для тишины. |
| Та же пустая «Сводка» в клиенте | [TUI render](../../../../crates/localvox-light-terminal/src/render/ux_tests.rs): `skipped_summary_has_its_reason_and_the_full_recook_command_in_the_document`; настоящий ConPTY проверяет skipped/reason и автоматическое появление нового summary без клавиши обновления. [Browser](../../../../ui/tests/e2e/workspace.spec.ts): `skipped summary explains the missing file and an idle document updates itself after one confirmed full recook` проверяет причину, отсутствие POST от чтения/отмены, единственный полный запрос от физической KeyR в другой раскладке и результат после очередного poll. |
| За пунктом 26 как будто ничего нет | [palette state/render](../../../../crates/localvox-light-terminal/src/app/navigation/tests.rs) проверяют страницы, начало/середину/конец, выбранную строку и диапазон на высотах 10/12/24/48; render — на 12/36. Палитра заполняет доступную высоту, показывает range/total, «ещё» сверху/снизу или «конец». ConPTY проверяет End → последняя команда, PgUp → предыдущая страница, Home → первая команда. |
| r непонятно обновляет данные | UI/TUI предлагают одну полную переварку записи. В TUI r → подтверждение s; Esc не отправляет запрос. В списке цель — выбранная видимая запись, в деталях — открытая. Цель захватывается при открытии подтверждения; соседний документ не очищается от ответа чужого recook. Поля ввода сохраняют буквальное r. Отдельные пользовательские scope и refresh убраны из палитры. Технический API фаз оставлен совместимым. Оба клиента обновляют открытый документ автоматически; UI сохраняет содержимое при чтении и пропускает poll скрытой вкладки. |

## Выполненные проверки

Native Windows, Rust tests с private fixtures/HTTP/ConPTY, browser Chromium:

| Команда / набор | Результат |
|---|---|
| `cargo test --locked -p localvox-light -p localvox-light-core -p localvox-light-api -p localvox-light-llm -p localvox-light-terminal -p localvox-light-ingest --lib`, финальная версия 0.2.10 | main **24 passed / 12 ignored**, core **497 / 11**, API **96 / 8**, LLM **113 / 0**, TUI **246 / 3**, ingest **7 / 1**; failed 0 |
| `cargo test --locked -p localvox-light-terminal --lib --test io_responsiveness --test pty_workspace`, `INSTA_UPDATE=no` | library **246 passed / 3 ignored**, I/O **3 passed**, ConPTY **10 passed**, failed 0 |
| Ingest unit/executable fixtures | **7 passed / 1 ignored**; ignored subprocess helper вызывается проверками таймаута/успеха/отказа |
| UI `npm test` | **16 passed** |
| UI `npm run test:e2e`, без обновления эталонов | **85 passed**, включая три ожидаемых отказа визуального детектора при намеренных мутациях символа/цвета/геометрии |
| UI `npm run build` | passed |
| `cargo check --locked --target x86_64-pc-windows-msvc -p localvox-light-asr -p localvox-light-youtube --bins` | passed; проверены вызывающие DownloadedAudio обработчики |
| `cargo build --locked -p localvox-light -p localvox-light-asr -p localvox-light-youtube -p localvox-light-terminal --bins` | версия 0.2.10, passed; vox/TUI `--version`, daemon `--help`, worker `--worker-protocol` passed |
| Настоящие установленные yt-dlp и ffmpeg, частный HTTP WAV на localhost | passed: заявленные аргументы создают непустой owned audio и действительный `after_move:filepath`; fixture каталог удалён при выходе |
| `test_release_delivery.py`, `test_ci_delivery.py` | **13 passed / 10 skipped**, **4 passed / 1 skipped** соответственно; Unix/complete-native opt-in не запускались |
| Strict main specs + change, `check-openspec.py`, API rustfmt, actionlint и diff | passed; main 15 capabilities / 88 requirements / 188 scenarios; версия согласована во всех 16 workspace packages, UI/lock и Tauri |

19 терминальных эталонов обновлены для подсказок auto-refresh/r и связанного
выравнивания. Пять browser эталонов сверены визуально: summary skipped, очереди
cancelled/failed, recording 800 и растущий partial. При 800 px обнаружено и
исправлено переполнение новых кнопок: перенос строки tabs, отдельная проверка
полной видимости кнопки и отсутствия горизонтального переполнения. Финальные
прогоны эталоны не меняли.

Финальный ConPTY на 0.2.10 дополнительно проверяет настоящий ввод r → Esc (POST
отсутствует), затем r → ss (один POST `scope=all` для выбранной записи). Ingest
fixtures добавлены в обязательный workflow Interface contracts and visuals.

Локальные логи: `target/review/recovery-20261123/`. Временные каталоги, Cargo/Rustup,
ORT cache и результаты сборки размещались на D:. Для browser использован уже
существующий Chromium; новые веса/браузеры не скачивались. Пользовательские архив,
настройки и работающий демон не изменялись.

## Не проверено этой работой

Native macOS/Linux клавиши/процессные группы/сеть; физический захват микрофона и
системного звука; качество ASR/LLM на пользовательской Mac-записи; публичная
установка нового релиза. Игнорируемые hardware/model/opt-in интеграции не выдаются
за выполненные. Исторические отчёты и принятый main не переписаны; новые требования
остаются в delta данного change до отдельной синхронизации.

## Hosted follow-up после тега

В run `45253208955` (main, 57e3b3f) browser получил **84 passed / 1 failed**:
старый `the final document refresh survives processing finishing during a slow read`
после 6,1 с требовал ровно два чтения. Это требование прекращать чтение после Done
противоречит принятому здесь автоматическому обновлению открытого документа.
В локальном полном прогоне старый assert прошёл из-за порядка async ответов;
в отдельном release run тот же UI этап также прошёл. Эти результаты не стираются.

Тест заменён управляемыми барьерами: первый медленный ответ, финал после Done,
затем третий медленный idle-read. Пока он задержан, последующие timer ticks
не создают параллельный запрос; после разрешения новый документ виден без кнопки.
Исправленный сценарий локально прошёл **20 повторов** с двумя workers. Эталоны и
исполняемый код 0.2.10 не изменялись; тег не переносился. Проверка данного delta
добавлена в обязательную строгую валидацию CI.

## Продолжение, 2026-11-24

Релизный run [45253209227](https://github.com/cascad/localvox-light/actions/runs/45253209227)
завершился **success** и опубликовал v0.2.10. Пять native платформ и Release
interface checks прошли. Отдельный branch Build binaries 45253209290 — success;
старый branch Interface 45253208955 — failure по описанному выше assert.
Это не аппаратная приёмка записи на Mac.

Последующий локальный полный browser прогон получил **84 passed / 1 failed**
в `recorder keeps stop available before confirmation and imports open their accepted session`:
после быстрого Stop запись снова выглядела активной. Воспроизведена реальная гонка
двух POST, а не увеличен timeout/добавлен retry теста. Новый тест удерживает ответ
Start; после Stop на старом RecordBar видит два одновременных fetch (`start`, `stop`)
и падает на требовании порядка. Лог воспроизведения: `record-order-red.log`.

RecordBar теперь последовательно отправляет изменяющие команды. Stop доступен
во время Start, но ждёт завершения его POST, а не подтверждения захвата движком.
Проверка старого Start вытесняется поколением команды. Предыдущий отклонённый POST
не оставляет очередь заблокированной. US-002 и доказательства REC-001/003 обновлены.

После исправления:

- **45 passed**: по 15 повторов сценариев idle polling, задержанного Start/Stop и
  обычной записи/импорта с двумя Playwright workers;
- **20 passed**: по 10 повторов задержанного успешного и отклонённого Start;
- полный browser без изменения эталонов: **87 passed**, `browser-v0.2.11.log`;
- UI unit: **16 passed**; UI build passed;
- native Windows debug build daemon/worker/ASR/YouTube/TUI/vox: **passed** для 0.2.11,
  `native-v0.2.11.log`. Rust implementation от v0.2.10 не менялась, повторные Rust
  unit suites только ради номера версии не запускались; прежние результаты выше
  не переименованы в новые;
- запись физических устройств и качество моделей не проверялись.

v0.2.10 не перемещается и не переписывается. Новая правка и e2e follow-up выходят
как v0.2.11; hosted результат новой версии на момент этого локального отчёта ещё
не получен.

## macOS ARM64: follow-up терминального теста

Первый релизный запуск [45345786423](https://github.com/cascad/localvox-light/actions/runs/45345786423)
на 2090d80 получил отказ в job 138279971806: installer preview **11 passed / 1 failed**,
`review_back_preserves_custom_choices_across_profile_changes` немедленно проверял
`✓ Готовить сводки` после обнаружения активного заголовка. В параллельном main job
138279983857 **тот же сценарий и все 12 preview tests прошли на macOS ARM64 с тем
же SHA**. Локально исходный assert также прошёл; точная первопричина единичного
hosted отказа по сохранённому логу не установлена (экран при assert не печатался).

Тест теперь ожидает само проверяемое значение тем же существующим bounded wait
12 с, а при отсутствии показывает итоговый экран. Ожидание только заголовка не
считается доказательством готовности остальных значений. Это не снятие проверки
галочки, не увеличение timeout и не retry неудачного assertion.

[Preview fixture](../../../../crates/localvox-setup-ui/tests/preview.rs) дополнен
доставкой stdout обычными порциями и по одному байту: PTY — поток, границы кадра
транспортом не гарантируются. Полный preview: **12 passed**; целевой сценарий
**10 повторов passed**, каждый в двух вариантах фрагментации. Инсталлятор и другие
исполняемые файлы от v0.2.11 не менялись; повторные native сборки моделей ради
test-only правки не запускались. Повтор hosted job и публикация остаются отдельной
проверкой задачи 3.3.

## Browser harness: проверка поставляемой сборки

Main run 45349843914 на cedfdd7: **86 passed / 1 failed** при первоначальном
`workspace()` до выполнения сценария отмены. Скачанный trace показывает отказ
загрузки `/src/main.tsx`: Chromium `net::ERR_NO_BUFFER_SPACE`; HTML и Vite HMR
отвечали, приложение не загрузилось. Это отдельный сбой стенда, а не подтверждение
ошибки логики отмены. Артефакт сохранён только в `target/review/recovery-20261123/`.

Полный browser suite переведён на production dist через
[Vite preview](https://vite.dev/guide/cli.html) и обычный
[Playwright webServer](https://playwright.dev/docs/test-webserver). Dev HMR и
раздача множества отдельных исходных модулей для него не требуются. `npm run test:e2e`
через стандартный pretest сначала выполняет typecheck/build; в CI отдельный
дублирующий build убран. Timeout, число retries и допуск изображений не увеличены.

Development-only StrictMode не потерян: отдельный `npm run test:e2e:dev` проверяет
сериализацию медленного чтения, обновление после Done и отмену запросов четырёх
вкладок. Его сервер использует другой порт, отчёты и traces — отдельные каталоги;
это исключает пересечение двух серверов при последовательном завершении на Windows.
Оба набора обязательны в CI.

Локально: production **87 passed**, StrictMode dev **6 passed**, эталоны не
менялись. В коде поставляемого приложения изменений нет; v0.2.11 не перемещается.
