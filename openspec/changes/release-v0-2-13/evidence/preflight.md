# Локальное исправление Unicode extraction · 2026-11-25

Продолжение [неопубликованного v0.2.12](../../release-v0-2-12/evidence/not-published.md).
Используется новый тег; история предыдущего не изменяется.

## Причина и регресс

Windows ANSI code page этой машины — 1251. Первый fixture содержал `Ю`, поэтому
проходил здесь и падал на английском hosted runner. Путь расширен до
`project with spaces Ю 漢字`: китайские символы не представлены ни в CP1251,
ни в CP1252. Точный предыдущий `setup-mpv.mjs`, взятый из коммита `2829e5c`,
в изолированном каталоге воспроизвёл `tar.exe: Error opening archive` локально.

Теперь tar получает только `-xf mpv.zip`; рабочий каталог передаётся через
Unicode API запуска процесса (`cwd`), без ANSI-преобразования полного пути.
Новое/готовое состояние проверяют тот же настоящий Node-скрипт и native tar;
обе ветки публикуют проверенный абсолютный `LOCALVOX_MPV`.

## Выполнено

| Проверка | Результат |
|---|---|
| Exact previous script + расширенный fixture | ожидаемый отказ воспроизведён; `release13-unicode-red-confirmed.log` |
| Новый setup + fresh/cache fixture | 1 passed; `release13-unicode-after.log` |
| Официальный кешированный ZIP с проверкой SHA → Unicode-каталог → настоящий mpv | passed: IPC, HTTP Range, pause/seek, null audio; 1 media test, 6.72 s; временный комплект удалён |
| Python `test_*delivery.py` | 86 run, 20 skipped, 0 failed; `release13-delivery.log` |
| Python `test_behavior_runner.py` | 9 passed |
| `cargo metadata --locked --no-deps` + Cargo/UI/Tauri/lock | все 16 workspace packages и манифесты 0.2.13 |

Логи — в игнорируемом `target/review/`. Остальные продуктовые проверки из
[preflight v0.2.12](../../release-v0-2-12/evidence/preflight.md) не повторялись
и не выдаются за прогоны v0.2.13. Между этими версиями изменены только setup mpv,
его регресс и метаданные выпуска; Rust/React поведение не менялось.

Новые hosted builds, interface gate, публикация и remote assets — pending.
Реальные Mac/Linux устройства, разрешения, качество/скорость ASR/LLM — not-run.
