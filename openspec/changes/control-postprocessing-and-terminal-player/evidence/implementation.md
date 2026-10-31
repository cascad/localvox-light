# Проверки рабочего дерева · 2026-10-31

Исторический прогон первой итерации с тремя переключателями. Текущая реализация
после уточнения владельца — STT и существующая опциональная summary;
её проверки записаны [отдельно](transcript-summary-timeline.md).

Изменение реализует намерение владельца: отдельные флаги после GigaAM в UI/TUI,
понятные пропуски рядом с журналом и терминальный плеер. Выбор владельца:
завершить текущий LLM-этап, сохранить результат, отключить ещё не начатые.
Это результаты исходников и временных fixtures, не обновление работающего демона.

## Реализация и связь требований

| Требование delta | Код | Проверка |
|---|---|---|
| CFG-004 | [каталог](../../../../crates/localvox-light-core/src/settings.rs), [приём URL](../../../../crates/localvox-light-core/src/ingest.rs), [UI](../../../../ui/src/Settings.tsx), [TUI](../../../../crates/localvox-light-terminal/src/app/forms.rs) | live только для трёх флагов, корректный default refine=off; settings HTTP сохраняет три off и новый URL получает три false; UI/TUI preset можно отменить |
| PIPE-001/002/015 | [очередь](../../../../crates/localvox-light-core/src/jobs.rs), [координатор](../../../../crates/localvox-light/src/autocook.rs), [схема](../../../../crates/localvox-light-core/src/workflow.rs), [API](../../../../crates/localvox-light-api/src/archive.rs), [UI](../../../../ui/src/Progress.tsx) | 8 сочетаний флагов; Running/manual/legacy; отказ публикации; подтверждённый STT сохраняется; действующий refine виден как running, следующие отключённые этапы как skipped |
| ARCH-006 | [backend](../../../../crates/localvox-light-terminal/src/player.rs), [клавиши](../../../../crates/localvox-light-terminal/src/app.rs), [полоса](../../../../crates/localvox-light-terminal/src/render.rs) | реальные mpv/Range/pause/fragment; формы не перехватываются playback-хоткеями; полоса на 160×48, 80×24, 64×12 |

CFG-001/002, ARCH-001/002/003, PIPE-008/011/012 и API-001 также затронуты регрессией.
Основные спецификации не sync/archive: delta находится в этом change; прежняя
аппаратная задача add-technical-journal-workspace/10.2 остаётся not-run.

## Выполненные проверки

| Команда / сценарий | Результат |
|---|---|
| cargo test --locked -p localvox-light-core --lib --no-default-features | 318 passed |
| cargo test --locked -p localvox-light --bin localvox-light | 5 passed: в том числе отсутствие worker после подтверждённого STT и отключение во время refine |
| cargo test --locked -p localvox-light-client -p localvox-light-terminal -p localvox-light-process | 110 passed; 3 внутренних helper-теста запускаются родительскими тестами; отдельная реальная mpv-проверка ignored в обычном запуске |
| cargo test --locked -p localvox-light-api --lib | 56 passed, 6 ignored; settings helper выполняется в собственной копии процесса |
| npm --prefix ui run build | PASS, TypeScript + Vite |
| npm --prefix ui run test | 8 passed |
| npm --prefix ui run test:e2e | 32 passed, включая 3 ожидаемых отрицательных проверки обнаружения изменения символа/цвета/геометрии |
| cargo test --locked -p localvox-light-terminal --test media_playback -- --ignored --nocapture | 1 passed, настоящий mpv ≥0.38, null audio, синтетический часовой WAV |
| cargo test --locked -p localvox-light-api --lib -- --ignored --exact http::tests::cross_clients::shared_workspace --nocapture | 1 passed, 4.84 s: настоящий API, браузер и TUI; настройки включают сохранение пресета |
| cargo build --locked -p localvox-light -p localvox-light-asr --bin localvox-light --bin localvox-process -p localvox-light-terminal --bin localvox-tui | PASS, dev-комплект демона/worker/TUI |
| OpenSpec main specs / текущий change strict / check-openspec.py | PASS; 15 capability, 86 требований main, 168 сценариев, 86 строк baseline; новые ID учтены отдельной delta-таблицей coverage |

В daemon-тесте два часа представлены таймкодами синтетического текста; это проверка
ветвления, не замер распознавания. В тесте отключения во время refine модель заменена
производителем проверенного артефакта: вызван ровно один этап, raw сохранён,
refined подтверждён, cleanup/summary отсутствуют. Между двумя вызовами Text сохраняется
один lease и общий timeout. Ручной Text/Summary сохраняет свой явный запрос.

Реальный mpv проверяет pause, seek без load, снятие границы фрагмента, повтор клипа,
переключение сессии, ошибки аудио и сохранение конечной позиции. При коротком seek
на паузе допустимое отклонение показаний audio clock — 250 ms для null-backend;
это не обещание точности физического вывода. Сохранённые PNG/symbol+style snapshots
проверены перед явным обновлением baseline; обычные тесты baseline не принимают.

## Обнаруженный обрыв транспорта

В общем API-прогоне короткие POST иногда давали ConnectionAborted, включая смену
best и запись заметки. Диагностика не выводила URL, токен или тело запроса.
Клиент теперь освобождает сокет после ответа через max_idle_connections(0),
без раннего запроса Connection: close. Автоматический повтор POST не добавлен.
Регрессия — 4 клиента × 16 коротких импортов: ровно 64 сессии и задания.
После исправления выполнены пять последовательных полных API-прогонов по
56 passed, затем новый общий прогон; проверки независимости control/documents
также прошли. Связь обрыва с ранним полузакрытием — вывод по диагностике и регрессии,
не трассировка ядра Windows.

При параллельном запуске cross-client fixture и сборке media-теста Windows отказала
в замене исполняемого localvox-tui.exe, занятого нашей fixture. Media-тест в том
запуске не выполнялся; после завершения fixture последовательный запуск прошёл.

## Границы

- GigaAM/LLM на настоящей двухчасовой записи не запускались, численный выигрыш не измерен.
- Микрофон и физический выход звука не проверялись; работающий демон и архив владельца не менялись.
- Старые jobs без follow_settings сохраняют snapshot: происхождение старой ручной команды достоверно не восстановить.
- При обоих включённых проходах Text запускает два worker-процесса. Модель/провайдер,
  инструменты и таймауты остаются Restart; флаги постобработки читаются перед каждым этапом.
