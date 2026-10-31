# Матрица паритета UI / новый TUI

Источник: прочитаны `ui/src/api.ts`, `App`, `DocumentPane`, `RecordBar`, `Player`, `Queue`, `Notes`, `Ask`, `Settings` на 2026-10-31. Это целевой перечень действий по существующему UI, а не утверждение, что каждая серверная ветка уже соответствует принятому контракту. Старый TUI не является образцом реализации.

Каждая строка должна получить отдельные ссылки на проверки UI и нового TUI и фактический результат. При планировании все строки имели **planned / not-run**; текущие частичные проверки приведены ниже. Общие данные fixtures — синтетические; реальные названия/тексты встреч не копируются.

| ID | Действие существующего UI | Эквивалент в новом TUI | Что проверять помимо отображения |
|---|---|---|---|
| P01 | Архив, группировка/выбор сессии | Список с датами, фокусом и состоянием | Пусто/ошибка/запись/длинный заголовок, переход A→B без старого ответа |
| P02 | Сводка, источники утверждений | Документ и выбираемые цитаты | Отсутствующая/готовая/непроверенная сводка, переход к правильной записи |
| P03 | Реплики с timecode | Навигация по репликам и источникам | Текст/время/участник, воспроизведение выбранного диапазона |
| P04 | Читаемый текст, оригинал исправленной строки | Документ с раскрытием исходной строки | Связь с версией, omitted/rejected, копирование без служебных данных |
| P05 | Версии и best | Список версий и действие «сделать рабочей» | ID запроса, сохранность оригинального STT, отсутствие лишнего LLM |
| P06 | Язык сессии | Выбор языка и подтверждение полного повтора | Нет модели/тот же язык/отказ до инвалидирования |
| P07 | Участники, локальное имя | Список и форма переименования | Локальная сессия, ошибка занятого worker, без enroll между сессиями |
| P08 | Названия источников | Форма имени и сброс к default | source_id и пустое имя, источник не подменяет голос |
| P09 | Вставка URL → импорт | Поле/палитра импорта URL | Приём с ID → открытие хода; ошибка не становится успехом |
| P10 | Очередь, активная фаза, причина отказа | Очередь и журнал выбранной задачи | Серверный порядок, несколько активных линий, failed/stuck/retry |
| P11 | Повторы Summary/Text/All | Действия с именованным scope | Верный endpoint/scope, подтверждение, worker lease, текст без аудио |
| P12 | Три режима поиска | Поле поиска и переключение режима | Параметры, empty/error/degraded, переход к результату |
| P13 | Вопрос к архиву и citations | Панель чата с выбираемыми источниками | Ответ/ошибка/нет фрагментов, grounded flags, правильный seek |
| P14 | Создать заметку в слоте | Редактор заметки и выбор слота | Явный destination, ошибка доставки видна, черновик сохраняется |
| P15 | Читать/удалять заметки слотов | Список и именованное подтверждение | raw+source+slot, unreadable отличается от empty |
| P16 | «Спросить»: материал, prompt, provider | Многострочный редактор с явным provider | Ограничения ASK-001, body, переход pending/running/done/failed |
| P17 | История/ответ/удаление asks | Список истории и документ ответа | Возврат после ухода из раздела, отсутствие автоматического повторного POST |
| P18 | Start/stop записи с названием | Постоянная строка и действия записи | accepted/confirmed/timeout, stop доступен при узкой ширине |
| P19 | Pre-roll, voice capturing/last result | Подписи состояния в постоянной строке | Разделение записи/диктовки/готовности/ошибки сохранения |
| P20 | Окончательное удаление сессии | Диалог с именем и отдельным подтверждением | Esc не отправляет DELETE, confirm совпадает, активное владение сохраняет файлы |
| P21 | Копирование документа | OS clipboard adapter и явный результат | Нет системных сносок/баннеров, ошибка clipboard, текст не потерян |
| P22 | Provenance, doubts, confirm artifact | Служебные сведения и действие подтверждения | Сомнение не равно invalid; I/O-отказ не выглядит подтверждением (RV-016) |
| P23 | Предложить слот заметки через route | Палитра/кнопка «предложить слот» | Выбор остаётся у человека, 503/timeout не отправляет заметку сам |
| P24 | Файл/текст для asks, копирование ответа | Пользовательский путь к текстовому файлу/вставка, clipboard | Unicode/размер/ошибка чтения, имя файла, без PDF parser, явная отправка |
| P25 | Play/pause, позиция/длительность, waveform | Одна строка плеера с peaks и статусом | Настоящий media/IPC, pause, отсутствующее аудио/backend |
| P26 | Seek/nudge по записи | Клавиши шкалы и ввод timecode | Absolute/relative границы, HTTP Range, без полной загрузки перед seek |
| P27 | Реплика/поиск/citation → play fragment | Единое действие «слушать источник» | Остановка в конце, повтор той же реплики, смена сессии без старого звука |
| P28 | Каталог настроек/группы/типы/unset | Форма каталога, поиск поля, сброс | Только изменённые ключи, bool/number/enum/secret, default не становится override |
| P29 | Токен и light/dark/system | Настройка подключения/маскированный токен и тема клиента | 401/403, токен не в argv/log/clipboard, theme не пишет server env |
| P30 | Сохранение настроек, need_restart | Результат save и список ожидающих параметров | Сохранение до apply, ошибка сохраняет edits, snapshot не объявляется Live |

UI drag-and-drop соответствует явному пути/вставке файла в терминале; mouse hover соответствует фокусу/палитре. Эти замены способа ввода не отменяют пользовательское действие. Переход в браузер не засчитывается за реализацию действия в TUI.

Ни запись с микрофона, ни реальный вывод звука не подтверждаются одним снимком/подставным HTTP. Аппаратные проверки отмечаются отдельно. P01–P30 не заявляют покрытие всего backend: pipeline fault/retry/cancel/retention и известные RV проверяются по существующим specs и своим changes.

## Текущая сверка 2026-10-31

Команды, результаты и пределы — в [implementation](evidence/implementation.md).
Browser означает [Playwright с mock API](../../../ui/tests/e2e/workspace.spec.ts),
App — [чистые тесты нового TUI](../../../crates/localvox-light-terminal/src/app/tests.rs),
HTTP — [настоящий клиент/сервер на temp archive](../../../crates/localvox-light-api/src/http/tests/workspace.rs).
Таблица не объявляет полный паритет: `partial` относится только к перечисленным проверкам.

| ID | UI | Новый TUI |
|---|---|---|
| P01 | browser archive/resize/error/stale document A->B | styles/ConPTY/HTTP list, dated rows/loading/empty/error at three sizes, App stale A→B |
| P02 | browser summary/citations/provenance/confirm | partial: HTTP summary, styles, App copy/fragment |
| P03 | browser STT/timecode/fragment + native PCM decoder | partial: HTTP text/time, App fragment, real mpv; не единый screen→sound e2e |
| P04 | browser readable/original/clean copy | partial: HTTP readable, original/copy, semantic scroll |
| P05 | browser actual version ID and no recook POST | HTTP best/IDs, STT сохранён, best не создаёт LLM job |
| P06 | browser cancel/confirm language body; real STT inference not-run | partial: HTTP busy/missing audio, meta сохранена; полный STT с моделью not-run |
| P07 | browser local voice + source requests; no enroll | HTTP локальное имя/participant/STT/worker refusal; без shared enroll |
| P08 | browser source_id/name/empty reset | HTTP source_id/имя/сброс; readable сохранён |
| P09 | browser accepted import ID opens its Progress | HTTP принятый import ID + App открывает его Progress |
| P10 | browser mock order/multiple running/stuck/error/journal link | App order/stuck/scope + HTTP queue; долгий реальный worker not-run |
| P11 | browser explicit confirmation and summary/text/all scopes | App подтверждение каждого scope + HTTP Text/Summary без аудио и отказ All |
| P12 | browser three modes/Unicode encoding/empty/source navigation | App pending/empty/error/draft/timecode + HTTP lexical/semantic/hybrid; synthetic embeddings, не реальная модель |
| P13 | browser grounded response/source navigation; synthetic model | App draft/error/source/timecode + HTTP grounded chat/пусто без LLM/двойной отказ; mock model, реальный inference not-run |
| P14 | browser explicit slot/delivery error/draft during save | HTTP default/explicit slot/отказ записи, App draft/error; внешняя доставка not-run |
| P15 | browser raw/source/slot delete/cancel/empty/unreadable | HTTP raw/source/slot delete/empty/read error + App named confirm и TestBackend unreadable/error/empty |
| P16 | browser explicit provider/Unicode text/pending/error | HTTP explicit provider/input, limits, pending; no model inference |
| P17 | browser return to stored answer/copy/delete/cancel/no repeated POST | HTTP synthetic status transitions/history/delete + App возврат/late response; lost-ack transport |
| P18 | browser accepted vs confirmed/stop while starting/resize; real capture not-run | App accepted/confirmed/timeout + ConPTY F8/quit; реальный capture not-run |
| P19 | browser pre-roll/dictation/delivery error; real voice not-run | TestBackend recording/dictation/receipt error; аппаратный voice not-run |
| P20 | browser cancel/named confirmation/DELETE | App Esc/late ACK + HTTP named confirm/worker guard/delete |
| P21 | browser clean copy with a clipboard adapter; physical clipboard not-run | App clean copy/clipboard failure + adapter export no-overwrite; OS clipboard acceptance not-run |
| P22 | browser checksum error/confirm/provenance excluded from copy | metadata/copy, core + HTTP checksum/read/deny-write/refusal/success; RV-016 исправлен |
| P23 | browser route sends no note and rejects stale hints; synthetic model | App route не пишет, отбрасывает устаревшие hints; HTTP mock LLM причина/только существующий слот/отказ. Реальный inference not-run |
| P24 | browser UTF-8 Unicode file/name/provider and copy source/answer | настоящий file→HTTP: UTF-8/BOM/Unicode/limits/name/provider; App чистая копия |
| P25 | browser native PCM decoder/pause/unavailable; intercepted HTTP, no physical sound acceptance | real mpv/IPC/auth/pause/missing backend/audio + real API/peaks; physical audio not-run |
| P26 | browser Home/End/arrows/15s bounded seek with real PCM decoder | real HTTP Range 1800s; App absolute/nudge bounds; audible seek not-run |
| P27 | browser repeated fragment/end + search/citation navigation; no physical sound acceptance | App source selection + real IPC repeated fragment/end/session switch/worker |
| P28 | browser all catalogue types/only changed keys/unset/secret | App mixed types/changed/default/filter/unset/validation; настоящий settings HTTP в copied private process |
| P29 | browser 401/local masked token/clear/light/dark/system | ConPTY 401→masked token→reconnect, no replay stop; transport/ASCII/16-color/mono |
| P30 | browser save error retains edits/need_restart; actual settings HTTP checked separately | staged keys/error/need_restart + actual settings HTTP; RV-001/010 исправлены, hardware capture not-run |
