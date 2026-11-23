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
