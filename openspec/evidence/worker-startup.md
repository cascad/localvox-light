> Исторический отчёт этапа до принятия baseline. Актуальный статус контракта — в [baseline](../baseline.md), соответствие кода — в [матрице](coverage.md). Результаты ниже сохраняют исходную дату и объём.

# Проверка запуска совместимого обработчика

2026-09-27, Windows, ветка `new_course`, незакоммиченное рабочее дерево.
Отдельное исправление после миграции документации; capability остаётся
**draft / awaiting-review**. Связь: [RUN-002](../specs/runtime-settings/spec.md).

## Воспроизведение причины

В пользовательском логе демон вызывает `localvox-process --worker-protocol`,
обработчик отвергает неизвестный аргумент и выходит с кодом 2. Проверка файлов:
release-демон собран 27 сентября, release-worker — 16 августа. Корневой
`default-members` выбирает только пакет демона: обычный `cargo run` не обновляет
соседний worker. Это отказ preflight, до захвата заданий очереди.

## Изменение и проверки

- [scripts/run.ps1](../../scripts/run.ps1) собирает daemon и worker одним вызовом
  Cargo; при Desktop также окно. Реальные пути берёт из compiler-artifact,
  проверяет наличие и общий каталог. По умолчанию release, доступен dev.
- [Диагностика preflight](../../crates/localvox-light/src/autocook.rs) сообщает
  путь, ожидаемый/полученный протокол, результат процесса и исправление;
  успешная проверка тоже имеет INFO-запись. После отказа нужен restart демона.
- Реальный `scripts/run.ps1 -BuildOnly`: release-сборка обоих бинарников успешна
  (36.25 с); остаются два прежних warning в core (deprecated name, unused_mut).
- Новый release-worker: `--worker-protocol` → `localvox-worker/1`, exit 0.
  Новый release-демон: `--help` → exit 0.
- Отказ Cargo проверен подстановкой команды, возвращающей код 23: скрипт
  выбрасывает ошибку до запуска приложения и восстанавливает исходный cwd.
- PowerShell parser, rustfmt --check для autocook, git diff --check — успешно.
  OpenSpec `validate runtime-settings --type spec --strict --no-interactive`
  — успешно, только INFO о длине двух требований.

Ревизия проверенного кода, SHA-256:

| Файл | SHA-256 |
|---|---|
| scripts/run.ps1 | `78C4C9C96045FDAFA17AF3F9F5376A0FB65518D411D63B41291845D8C7CDE89C` |
| crates/localvox-light/src/autocook.rs | `FC7F3C1A416D0F6AB73D78458F32FC95E6F9B09A50233A2A3B2A3FECAA222A00` |

## Граница проверки

Демон с записью и реальная очередь видео не запускались. Скорость STT/LLM этим
исправлением не измерена. Варианты Desktop/dev, живая ошибка несовместимого
worker в новом демоне и весь supervisor повторно не прогонялись.
Старый [снимок миграции](code-snapshot.json) сохранён как историческое свидетельство,
не обновлён задним числом. Этот прогон не принимает capability как источник правды.
