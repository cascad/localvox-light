# Общий разрезатель, 2026-11-09

Этап 2.1. [windowing.rs](../../../../crates/localvox-light-core/src/windowing.rs)
вынесен из cook без ONNX-зависимости. Offline cook и benchmark используют тот же
публичный `Windower` через re-export. Capture thread, ASR cache и ранний документ
пока не подключены. Пользовательский справочник не объявляет будущие действия.

Сохранены 320-sample frames, max/min/silence параметры, soft cut, отступ к середине
наибольшей паузы, forced cut без паузы, порядок native VAD и flush последнего
неполного frame только при EOF. Recipe прежний. Убраны полный pending PCM и
`Vec::drain` с переносом всего остатка на каждом frame: pending теперь <320 samples,
основной вход читается срезами. Окно остаётся ограничено заданным max плюс frame.

`feed_all`/`finish_all` выдают speech **и silence** с целыми sample bounds; прежние
`feed`/`finish` передают ASR только speech. Silent диапазоны можно подтвердить без
модели; их PCM не исчезает из покрытия. После успешного EOF повторный finish пуст,
новый feed отказывает. Отказ emitter делает planner непригодным для продолжения:
нужен replay подтверждённого аудио, а не пропуск/двойная публикация окна.

## Проверки

[Тесты](../../../../crates/localvox-light-core/src/windowing/tests.rs) и
[детерминированный PCM](../../../../crates/localvox-light-core/src/windowing/fixtures.rs).

- До извлечения запущен настоящий старый `cook::Windower` на `00188a0`:
  `capture_legacy_window_oracle` — 1 passed. Временный генератор удалён; сохранены
  [числовые bounds/PCM SHA](../../../../crates/localvox-light-core/src/windowing/legacy-windows.json).
  Тесты не перегенерируют oracle. Новый speech-only результат совпал во всех шести
  fixtures: default, retreat, forced, silence, soft, subframe.
- 32 фиксированных случайных разбиения каждого fixture и куски 1/319/320/321/16073
  samples дали побайтово те же speech/silence окна. Полное покрытие проверяет
  каждый sample, отсутствие дыр/дублей и bounded хвост; это 222 расписания.
- Настоящий WebRTC VAD LowBitrate: ещё 12 случайных разбиений каждого fixture,
  72 расписания; порядок каждого полного входного frame и все PCM windows совпали.
- Слово через шов и неполный frame ждут следующую порцию/EOF. Пустые feed не
  закрывают окно. Ошибка публикации и EOF retry не дают скрытого продвижения.

Все пять новых model-free tests passed без ONNX. Прежние cook tests сохранены,
включая quietest cut, carried tail, silence/timecode, forced cut и recognition noise.
Полная core серия с ONNX: 422 passed, 6 ignored. Benchmark/application dependency
graph: cargo check passed; публичный re-export совместим. Rustfmt/diff clean.
Strict main: 15 passed / 0 failed; change valid; check-openspec: 88 requirements,
188 scenarios, 88 coverage rows, 1616 local links PASS.

PIPE-015/016: проверен только общий input planner и покрытия; worker/cache/checkpoint
ещё not-run. Принятый PIPE-004: регрессия разрезания, без нового native ASR прогона.
Длинная запись, reference budget, устройства и слабый Mac остаются задачами 7.2/7.4.
