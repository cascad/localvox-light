# Локальная подготовка v0.2.12 · 2026-11-25

Windows, Rust 1.99.1, один Cargo worker и один test thread. Синтетические архивы,
PCM и приватные HTTP/PTY; рабочая установка, пользовательский архив и устройства
записи не использовались. `skip_specs`: версия и тестовые/CI инструменты;
продуктовые изменения описаны предыдущими change.

## Найденные отказы

- Предыдущий [interface run 45409700217](https://github.com/cascad/localvox-light/actions/runs/45409700217)
  упал в media backend: mpv был скачан, но Cargo integration test не находил его
  из каталога crate. `setup-mpv.mjs` теперь экспортирует абсолютный `LOCALVOX_MPV`
  в `GITHUB_ENV` и при загрузке, и при проверенном cache hit.
- Дополнительный настоящий UI/TUI HTTP-сценарий локально упал на старом ожидании
  раскрытого учёта времени. Он теперь раскрывает учёт и этап сводки, проверяя
  баланс, отсутствие неизвестных часов и все 35 строк журнала. Повтор: passed.
- URL fixtures явно отключают managed tool updates, чтобы не подменять свои
  программы и не обращаться к внешнему сервису. Транспорт обновления имеет
  отдельные тесты. Быстрый HTTP size-limit тест получил отдельный timeout 2 с;
  deliberate slow-response тест сохраняет 50 мс.

## Выполненные проверки

Логи находятся в игнорируемом `target/review/release-*.log`.

| Команда / группа | Фактический результат |
|---|---|
| Python unittest `test_*delivery.py` | 86 run, 20 skipped, 0 failed; включая настоящий Node fresh/cached export с SHA ZIP fixture, пробелами и Unicode в пути |
| Python `test_behavior_runner.py` | 9 passed |
| Python `test_model_sources.py` | 8 passed |
| `check-model-sources.py` | 15/15 публичных URL доступны; HEAD/ограниченный Range, без полной загрузки весов и проверки их hash |
| Cargo root `--lib --bins --test setup --test install --test model_paths` | 52 passed, 12 ignored; модельные/аппаратные ignored не выдаются за executed |
| Cargo installer UI crate | 14 passed, doc tests 0 |
| Cargo ingest `--lib` | 14 passed, 2 ignored |
| Cargo root `autocook::cancellation_tests --ignored` | 2 passed: owned child cancellation |
| Cargo terminal `--test media_playback --ignored` | 1 passed: настоящий mpv IPC, HTTP Range, pause/seek, null audio |
| Cargo API `http::tests::cross_clients::shared_workspace --ignored --exact` | 1 passed после исправления ожиданий: настоящий браузер + TUI + общий HTTP/архив |
| Сборка пяти Windows CLI binaries | passed, dev profile; не утверждение о release archives |
| Cargo/UI/Tauri/lock metadata | 0.2.12, все 16 workspace packages согласованы, `cargo metadata --locked` passed |
| API rustfmt, строгая валидация 15 main specs и release change, check-openspec, diff | passed |

Предыдущие продуктовые проверки остаются в evidence соответствующих change;
этот отчёт не превращает их в повторные прогоны текущей версии.

## Ещё не выполнено на момент локального preflight

Пять hosted native builds, tag preflight, release interface gates, публикация
и проверка 14 release assets — pending до отправки тега и фактического результата
Actions. Нативные Mac/Linux пользовательские сценарии, реальные разрешения,
микрофон/loopback и качество/скорость моделей — not-run локально на Windows.
