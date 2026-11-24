# Локальные проверки поведения · 2026-11-24

## Контракт и исправления

- ARCH-001/002/005 и delta ARCH-024: `runtime` больше не маркирует глобальные
  запросы поколением 0. `Io` сохраняет поколение ответа и один pending запрос
  ресурса, но не отменяет глобальную операцию при смене документа. Deletion fence
  продолжает отбрасывать старые ответы. Проверены свежий poll после удаления,
  удаление последней записи, повторный title и запоздалый текст прежнего ID.
- ARCH-024: атомарно создаваемый каталог новых сессий содержит UUIDv7; одинаковое
  название/секунда после удаления не переиспользуют ID. Старые каталоги не мигрируют.
  API сохраняет проверку ownership, остановку worker и удаление файлов/очереди;
  служебный durable tombstone защищает от восстановления удалённого задания.
- REC-002 меняется по прямому поручению владельца: удалено кольцо архивного
  pre-roll, сдвиг started_at и настройка/индикатор буфера. Legacy env/CLI принимает
  значение, но не добавляет историю. `RecordState.preroll_sec = 0` для старых клиентов.
  Bounded orphan на 60 секунд относится только к уже принятому Start.
- ARCH-025: подтверждение без полей — s/ы; однострочные параметры — Enter,
  многострочный текст — Ctrl+Enter. Старые Control-сочетания остаются совместимыми
  alias, а не обязательным способом. Палитра принимает n/p, т/з и Ctrl+n/p;
  в редакторе буквы не становятся командами.
- ARCH-002: mpv дожидается загрузки перед управлением; global `options/end`
  доступен и при idle, в отличие от `file-local-options/end`. После EOF Toggle
  загружает аудио с конца фрагмента либо с начала полного файла. Настоящие ошибки
  процесса/декодера остаются ошибками. Диагностика плеера содержит сессию без
  посторонней текущей фазы ASR.

## Существенные сценарии

`app/tests.rs`: `deleted_record_is_not_restored_by_a_later_stale_global_list_response`,
`deleting_the_last_record_and_recreating_its_title_does_not_inherit_the_old_document`,
`palette_n_p_ctrl_n_p_and_russian_positions_navigate_without_dispatching`,
`start_name_keeps_s_and_n_p_as_text_and_submits_with_enter`,
`playback_errors_keep_the_session_without_claiming_the_current_pipeline_phase`.

`io.rs`: `global_reads_keep_their_generation_and_survive_document_changes_without_duplicate_polls`.
Native PTY: `record_deletion_from_list_confirms_the_pinned_target_and_stays_responsive`
проверяет удаление и новую строку с прежним title через естественный poll без restart.

`pipeline.rs`: `twenty_seconds_between_commands_stays_twenty_seconds_on_both_tracks`
читает реальные WAV: по 320000 сэмплов каждого источника, только значения между
Start/Stop. Проверены legacy 300 секунд, idle PCM, короткая ротация и восстановление
orphan после отказа каталога. Это synthetic PCM, не аппаратная запись.

`media_playback.rs`: настоящий mpv, локальный HTTP Range/WAV, null audio;
Load→немедленный Toggle, pause→seek→resume за старой границей, fragment EOF→Toggle,
повторные фрагменты и отказ после потери процесса. Модель/звуковое устройство не нужны.

## Общий runner

`scripts/test-behavior.py` — фиксированный manifest для Windows/macOS/Linux.
Один worker по умолчанию, ограниченные логи и время, временные HTTP/архив/PCM/IPC,
отдельные failed/blocked/timeout/not-run. Параметр `--installed-dir` явно выбирает
настоящие PTY-клиенты и готовые native libraries; остальные тесты относятся к source.
`AGENT_TASK.md` содержит платформу/commit, реальные отказы, argv, cwd и логи.
Автоматической отправки или запуска агента нет.

Девять контрактов runner выполнены: реальный child success/failure/timeout,
недостающая зависимость, ограниченный лог с хвостом отказа, запрет false-green
для пустого/ignored Cargo, изоляция env, manifest, отчёт/handoff и reuse библиотеки
для трёх имён платформ. Последнее — проверка resolver на fixtures, не три native прогона.

Обычный профиль сохраняет browser PNG и выполняет поведенческие assertions,
не сравнивая шрифты с чужой ОС. `--visual` включает строгие эталоны и проверки
чувствительности детектора; обычный CI не ослаблен. Просмотрена единственная
изменённая картинка `queue-unavailable-dark-win32.png`: удалён только ненужный
индикатор «буфер 05:00».

## Прогоны и границы

Промежуточные отказы сохранены в `target/behavior/20261124T121821-10844` и
`20261124T123719-5608`: устаревшая подсказка Ctrl+S в PTY-тесте и race чтения
финального installer receipt. Вторая проверка ждала завершение процесса раньше,
чем reader передал последние данные ConPTY. Теперь отдельно ожидается видимый
«LocalVox готов», затем exit status и содержимое установленного комплекта.
Условия установки и assertions файлов сохранены.

Итог: Windows native, `python scripts/test-behavior.py --visual`, **17/17 шагов passed**.
Отчёт: `target/behavior/20261124T124349-18944/report.json`, логи и AGENT_TASK рядом.
Использованы текущие исходники, worker limit 1, Rust test threads 1.

| Группа | Выполнено | Не выдано за passed |
|---|---:|---|
| TUI state/render, HTTP и настоящий ConPTY | 251 + 3 + 10 | 3 ignored library |
| Core lifecycle/storage/settings | 499 | 11 ignored; причины в логе |
| Incremental worker fixtures | 6 | Реальные веса не запускались |
| Archive API и journal HTTP | 33 + 1 | 2 ignored API вне этой группы |
| Setup commands и настоящий wizard | 49 + 13 | 12 ignored вне выбранных setup tests |
| Delivery scripts, локальные fixtures | 64 | 20 skipped для другой ОС/отсутствующей внешней инфраструктуры |
| Настоящий mpv IPC | 1 | Null audio, без колонок |
| Совместная работа настоящих GUI/TUI и API | 1 | Временный архив/API |
| GUI browser + development StrictMode | 87 + 6 | Строгие Windows PNG; модели/API browser fixtures |
| Runner contracts отдельно | 9 | Platform resolver fixtures не равны Mac/Linux native |

GUI unit (16 tests), build и сборка TUI также passed. Строгая валидация 15 main specs и этого
change, `python scripts/check-openspec.py` (88 requirements, 188 scenarios),
`git diff --check` и actionlint 1.7.12 для изменённого workflow — passed.
Незавершённый proposal-only `add-tone-sbert-asr` этим не реализован/не архивирован.

Фактические Mac/Linux прогоны, установленный release-бинарник, реальный микрофон,
loopback/разрешения ОС и качество/скорость моделей — **not-run** на этой машине.
Команды и границы: [behavior-testing.md](../../../../docs/behavior-testing.md).
