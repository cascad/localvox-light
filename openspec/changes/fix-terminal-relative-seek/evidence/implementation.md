# Перемотка TUI и Enter при полном выходе · 2026-11-24

Реализовано в исходниках и проверено на Windows. Delta не sync/archive;
публичная публикация и аппаратная приёмка Mac/Linux этим отчётом не подтверждаются.

## Причина и исправление

`App::nudge` вычислял абсолютную цель из последнего отчёта плеера. Пока новый
отчёт не пришёл, несколько Alt+→ посылали одинаковые 15 секунд. Теперь App
передаёт session/delta, а поток плеера накапливает принятую цель и отправляет mpv
absolute+exact. Отображается фактическая позиция mpv; заданная цель её не подменяет.
Load/Stop/Toggle/ошибка сбрасывают цель; воспроизведение снимает её, догнав переход.
Новая запись, EOF, границы и остановка фрагмента используют прежние механизмы.

Штатный relative+exact был проверен и отвергнут: на паузе три перехода дали
44.449021 вместо 45 секунд из-за накопления смещения аудиобуфера. Тестовый допуск
фактического clock остался прежним, 250 ms; точные команды и конец фрагмента
проверяются отдельно. Предварительные неуспешные прогоны не выдаются за успех.

В полном выходе Enter проходит общий Press-only фильтр подтверждений наряду с s.
Enter открытия палитры только открывает диалог; Repeat/Release/Paste не отправляют
shutdown. Повторное подтверждение во время запроса не дублирует его. Клиент ждёт
ответа демона. Enter остальных подтверждений остаётся инертным, текстовые формы
сохраняют своё поведение. Подсказка выхода и US-015/062 обновлены.

## Фактические проверки

Cargo использует `--jobs 1`, тесты — `--test-threads=1`; mpv запускается отдельным
владельцем с null audio и синтетическим HTTP-аудио. Пользовательский демон, архив,
запись и буфер обмена не используются.

| Проверка | Результат |
|---|---|
| `cargo test -p localvox-light-terminal --lib` | 253 passed, 3 ignored; подтверждения Enter/s/ы/совместимые Ctrl, Repeat/Release/Paste/Esc, исходная цель, pending, формы и рендер |
| `cargo test -p localvox-light-terminal --test pty_workspace` | 11 passed; ConPTY, настоящие клавиши Enter и s → один HTTP shutdown, восстановление терминала; также ввод, навигация, список/удаление и повторное подключение |
| `cargo test -p localvox-light-terminal --test media_playback -- --ignored` | 1 passed; настоящие App hotkeys → worker → mpv, задержанная телеметрия, 3×15=45, обратные/смешанные шаги, воспроизведение/пауза, новая запись и обе границы; прежние auth/Range/fragment/EOF/recovery проверки сохранены |
| `cargo build -p localvox-light-terminal --bin localvox-tui --bin vox` | passed |
| Полный локальный bundle через `scripts/package-release.py` | пять startup smoke и bundle integrity; без личного `.env` и весов |

Логи локальных прогонов: `target/review/tone/controls-lib.log`,
`controls-pty-all.log`, `relative-seek-final.log`, `controls-build-final.log`,
`controls-bundle.log`. Mac/Linux native input/mpv и аппаратный звук: **not-run**.

Строгая проверка main specs: 15 passed; change: valid; `scripts/check-openspec.py`:
PASS (15 capabilities / 88 requirements / 188 scenarios / 2258 local links);
`git diff --check`: passed. CLI отдельно сообщает, что archive RUN-006/007 требует
предшествующей синхронизации `unify-client-confirmations`: эти требования ещё
не находятся в main. Archive/sync не выполнялись.

Локальный комплект: `target/review/controls`. Рабочая установка `F:\lv` этим
этапом не заменялась; комплект готов для явного запуска установщика.
