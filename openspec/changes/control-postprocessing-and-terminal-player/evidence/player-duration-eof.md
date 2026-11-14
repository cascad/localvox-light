# Длительность TUI-плеера при EOF · 2026-11-15

Контракт: известная длительность в ARCH-006 этого change; границы фрагмента ARCH-002.
Нормативный текст не ослаблялся и не менялся ради дефекта.

## Hosted failure

На `71b301c` (документация; player code тот же, что в `4514678`) все пять
[Build jobs](https://github.com/cascad/localvox-light/actions/runs/43636185474) прошли.
[Interface job](https://github.com/cascad/localvox-light/actions/runs/43636185386/job/132573386097)
завершился failure в `Real media backend (generated PCM, null audio output)`:

```text
real_player_auth_range_pause_and_repeated_fragment
media_playback.rs:300: assertion failed: finished.duration.is_some()
```

`shared_workspace` перед ним прошёл. Последующие шаги этого Interface run не
выполнялись; весь workflow не считается успешным. Старый зелёный run на `4514678`
не переписывается как failed, опубликованный v0.2.0 остаётся прежним.

## Причина и correction

В `Mpv::state` pause, idle-active, duration и time-pos читаются отдельными IPC
командами. При переходе EOF idle-active может ещё вернуть false, а duration уже
стать недоступной. Прежний `Backend::poll` сохранял metadata только для idle=true.
Промежуточный active/None затирал известное из playback API значение, следующий
idle восстанавливал уже None. Точный wire transcript hosted runner не записан;
путь потери подтверждён отдельной детерминированной regression fixture.

Слияние телеметрии вынесено в приватный `Backend::update_state`: отсутствие нового
duration сохраняет известное значение текущей сессии. Валидное новое значение
по-прежнему обновляет длительность. Настоящий Load сбрасывает прежнюю identity и
metadata до запуска backend/API, Stop очищает состояние; значение не переносится
между записями. Конечная позиция/пауза фрагмента и правила seek сохранены.

## Проверки

Windows x64, Rust 1.99.1; cache/TEMP/build на D:, пользовательский демон/архив
и аудиоустройства не запускались и не менялись.

1. До исправления после извлечения прежней логики без изменения поведения:
   `cargo test --locked -p localvox-light-terminal --lib player::tests::eof_between_ipc_properties_retains_duration_and_new_load_clears_it --target x86_64-pc-windows-msvc -- --exact` —
   **1 failed**: фактическое None вместо Some(3600), без timing/sleep/network.
2. После исправления вся TUI lib: **213 passed / 3 ignored**. Regression проверяет
   active/None → idle, повторный idle, endpoint, session/snapshot identity, настоящий
   superseding Load с отменённым startup, новую duration=90 и Stop.
3. Неизменённый исходный `media_playback` acceptance с реальным mpv, generated PCM,
   localhost HTTP Range, private IPC и `--ao=null`: **5/5 consecutive passed**.
   Проверяются auth, range/ограниченное чтение, pause/seek, повтор фрагмента,
   новая сессия, duration/endpoint после EOF, Stop и восстановление после HTTP error.
   Это повтор для проверки обнаруженной гонки, а не retry-to-green в workflow.
4. Assertions/таймауты native acceptance не ослаблены, sleep/retry в приложение
   или CI не добавлены. Физический звук/микрофон и новый hosted run — **not-run**.
5. Обе TUI binaries собраны в dev profile; rustfmt, strict main specs (15/15),
   strict change, `check-openspec.py` и scoped diff прошли. Полный release/republication
   не выполнялись; это локальный patch, требующий отдельного нового hosted результата.

Локальные протоколы: `target/tmp/player-duration-red.log`,
`target/tmp/player-duration-green.log`, `target/tmp/player-duration-native-1.log`
… `player-duration-native-5.log`. Generated WAV передаётся потоково; настоящие
записи/модели не использовались.
