# ARCH-018 · Детали оформления, 2026-11-11

Референс владельца — оформление Termide, без переноса расположения панелей или git.
Изменены только Theme/render и style baselines; навигация, данные, документы,
копирование и позиция воспроизведения не меняются. ARCH-018 остаётся delta,
main specs не синхронизированы.

## Оформление

`Theme.focus` отделён от command/value. Активная рамка и технический title — тёплые,
неактивные рамки — приглушённые. Реальное название записи сохраняет основной цвет
данных. Тёмный фон слегка смещён в navy, выбранная строка получила спокойный тёплый
фон; светлая тема согласована с ним. Palette/form/help/console используют тот же
акцент. Семантические success/error/busy цвета сохранены.

В 16-color применяются штатные Yellow/Cyan/DarkGray, без RGB; mono сохраняет
`>` активной панели, начертания и reversed selection. Настройки темы и действия
остаются прежними. Пользовательский сценарий — US-039.

## Test-run

- **190 library passed / 3 helpers ignored**, включая actual Buffer для четырёх
  focus panes × четырёх тем, overlay title/border и различие технического title,
  label, hint, реального значения и текста. RGB contrast text/label/value/focus
  на основном фоне и selected row **≥4.5**.
- **38 style snapshots обновлены после review**. Сравнение каждого `Buffer.area`
  и всего `Buffer.content` с HEAD подтвердило побайтное равенство текста/геометрии.
  Изменились styles; смена стиля создаёт новые границы runs в debug snapshot.
  Впервые запущенный `INSTA_UPDATE=no` закономерно нашёл старые стили; после
  проверки diff повтор с обновлением выключенным прошёл.
- **5 actual ConPTY passed**: paste/resize/quit/restore, partial growth без сдвига
  старой строки, фильтр/activity/RU commands, keymap/reconnect и invalid config.
  **2 IO / 2 run-script passed**, hardware mpv ignored.
- UI production build прошёл в общем комплекте; этот change UI не меняет.

Команда: `cargo test --locked -p localvox-light-terminal --jobs 2 -- --test-threads=2`
с `TEMP/TMP` на D: и `INSTA_UPDATE=no`. Фактический итоговый прогон входил в общий
multi-package test; ignored логи `target/tmp/warm-asr-verified-regressions.log`,
`focus-snapshot-content-check.txt`, `focus-snapshots-update.log`.

Эстетическая оценка владельцем в его терминалах, Mac и аппаратный звук **not-run**.
ANSI цвета могут зависеть от палитры самого терминала; эмулятор не определяется
по имени и отдельной логики для WezTerm нет.

Release TUI собран, `--version` прошёл. SHA256:
`ceeb091f775db0dc8dc642482ef6b167309d657e612f926258f6c20c830c59a6`.

Strict main: 15 passed / 0 failed; strict change valid; `check-openspec.py`
PASS (15 capabilities / 88 requirements / 188 scenarios). Staged diff проверен,
в коммит входят только оформление, его snapshots/контракт/evidence и US-039.
