# Восстановление импорта и обслуживание yt-dlp · 2026-11-25

## Реализация и границы контракта

PIPE-023, RUN-008 и SETUP-012 находятся в delta этого change по поручению владельца.
Main specs не синхронизированы. TERM-009 сохраняет исходную норму компактного
индикатора ошибок вне консоли; обратная связь не выдаёт stderr за текущую работу.

- Recook с валидной сохранённой HTTP(S)-ссылкой допускается до появления аудио.
  Сохраняются сессия/kind, durable очередь, проверка занятости, rollback метаданных
  при отказе постановки и конечный бюджет. Новые автоматические возобновления не добавлены.
- Полная причина вынесена в необязательное `execution.error`, короткий `message`
  остаётся состоянием. Следующие этапы не копируют предыдущий stderr. Известный
  отказ download не превращает extract в ещё один отказ той же фазы.
- В UI причина раскрывается один раз в ограниченном блоке, в TUI — в консоли с фазой.
  После HTTP-отказа пустое подтверждение не восстанавливается; явный новый action
  разрешён. Заметки/настройки сохраняются, отдельный контракт shutdown не меняется.
- Управляемый standalone yt-dlp размещается в `~/.localvox/tools/yt-dlp/<platform>`.
  Внешний executable не меняется. Официальные stable tag и SHA2-256SUMS, SHA-256,
  запуск `--version` предшествуют атомарной замене state-указателя на уникальную
  проверенную папку. Кешируется сутки; отказ проверки — час. File lock сериализует
  публикацию, ожидание ограничено; остаются текущая/предыдущая управляемые копии.
  Чужие каталоги/ссылки не удаляются, цели очистки проверяются через canonical paths.
- Сетевые ответы, время и размеры ограничены. Повреждённый кеш не выбирается;
  новая проверенная копия может заменить указатель без перезаписи занятого exe.
  При отказе сохраняется рабочий fallback. Doctor не запускает сетевое обновление.
- AUTO_UPDATE включён в `.env.defaults` и виден в настройках с Live-областью.
  Выключение не выполняет проверку обновления. Подключены queued, standalone ASR
  и legacy YouTube URL-entrypoints; локальные файлы не вызывают updater.

## Фактические проверки Windows

| Проверка | Результат | Лог в target/review |
|---|---|---|
| Core с ONNX feature | 517 passed, 14 ignored | import-core-api-final.log |
| API | 99 passed, 8 ignored | import-core-api-final.log |
| TUI state/render/unit | 260 passed, 3 ignored | import-tui-last.log |
| Настоящий Windows ConPTY | 13 passed | import-native-pty.log |
| Ingest/updater: SHA, cache/backoff, publish, concurrency, corruption/repair, retention, platform/off и настоящий HTTP timeout/size | 14 passed, 2 ignored | import-ingest-last.log |
| Настоящее скачивание и запуск официального yt-dlp | 1 passed: 2026.11.23, SHA и `--version`, 4.91 с | import-ytdlp-native.log |
| UI unit и production build | passed | import-ui-unit.log, import-ui-build-last.log |
| Полный browser behavior набор | 87 passed | import-browser.log |
| Новый browser-сценарий и просмотр PNG после исправления layout | 1 passed; полный stderr один раз, изначально скрыт, escaped, заголовок над ограниченным pre | import-failure-final.log |
| Полная сборка и локальный комплект | 6 бинарей собраны; финальные 5 поставляемых CLI прошли smoke, worker protocol и bundle-check | import-build-last.log, import-final-build.log, import-bundle-final.log |
| Rustfmt / strict specs/change / check-openspec / diff | passed; 17 Rust-файлов, 15 main specs; main 88 requirements/188 scenarios/88 coverage rows | import-format.log, import-main-validation.log |

API regression проходит отказ prepare трижды, затем recook до аудио: тот же ID,
prepare/attempts=0/force, исходный ingest-kind и защита уже запущенного задания.
Отсутствующий/неверный источник не уничтожает прежнюю сводку или метаданные.
TUI regression различает вызовы recook и фоновые GET: после отказа нет повторного
POST и пустого диалога, следующий Enter не подтверждает повторно. ConPTY повторяет
это через настоящий HTTP 400 и открывает полную причину клавишей F12.
Проверены новый явный повтор удаления и сохранение изменённых настроек после отказа;
текстовые черновики и shutdown остаются покрыты прежними сценариями.
Отдельный state-регресс проверяет фазу structured error, отсутствие нового unread
от того же polling и новый эпизод после recovery либо смены attempt identity.

Первичные несовпадения тестов (новая Live-настройка и прежняя политика восстановления
пустых форм) исправлены по новому контракту с сохранением проверок цели/черновика.
Ошибки компиляции новых вызовов parser/logging пойманы локально до поставки.
Перечислены только отдельные итоговые успешные логи; первые прогоны не являются успехом.

## Не проверено

Native Mac/Linux и аппаратный звук/модели — not-run. Живой YouTube-импорт с аккаунтом
не выполнялся: HTTP 429 и запрос авторизации не доказывают устаревание программы,
обновление не обещает снять ограничения источника. Cookies браузера не читались.
Проверки использовали временные данные на D и официальный публичный релиз;
личная установка/архив не изменялись.

Финальный комплект: `target/review/import-recovery`. Все пять бинарей, DLL, mpv,
обновлённый frontend и `.env.defaults` включены; пользовательская `.env` и модели
не копировались. Предварительный сгенерированный комплект заменён после проверки
нового и проверки абсолютных путей; F:\lv не затрагивалась.

Источники: [официальные релизы](https://github.com/yt-dlp/yt-dlp/releases),
[официальный FAQ по HTTP 429](https://github.com/yt-dlp/yt-dlp/wiki/FAQ#http-error-429-too-many-requests-or-402-payment-required).
