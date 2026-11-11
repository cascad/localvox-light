# Включение default · план контроля до запуска, 2026-11-11

Владелец прямо поручил: «прогони и включи по дефолту, в настройках можно пусть
будет выставить». Продолжается 7.3 существующего change: PIPE-018 уже требует
видимый default on и сохраняемый переключатель. Нового ASR/LLM поведения нет.

## Контроль для этого решения

- Полный публичный PCM: 114344438 samples / 7146.527375 s на дорожку, SHA
  `96e50ce45af3988b3b069c0ebd4363842eb06b3fe5b536490a8dc9d81f02cd46`.
- Тот же текущий capture/planner в обеих сторонах. O распознаёт после Stop,
  I распознаёт finite prefixes и дорабатывает final. Это сравнение именно настройки
  сегодняшнего приложения: capture ledger/planner уже существуют и при off.
- Native GigaAM int8/vocab, ru, recipe r7 / 15/8/500, diarization/LLM off.
  Проверенный release worker из ff22c12; default не участвует в явных worker-phase
  запросах profiler. Порог и проверки не меняются: **≤20% в каждой паре и суммарно**,
  exact PCM windows/coverage, text и число ASR calls совпадают; по одному engine
  load/start на режим, готовые окна после Stop не распознаются повторно.
- Три alternating пары: O1/I1 steady300, O2/I2 burst900, O3/I3 steady300.
  Новые sessions/caches, весь вход, без --limit-sec; --current-capture-baseline.
  Сборки и другие наши ASR во время серии не запускаются. TEMP/TMP и outputs на D:;
  рабочий daemon/личный архив/устройства не используются.
- До/после сверяются SHA input/worker/weights/vocab, отдельно сохраняются
  producer/worker wall, CPU, logical I/O, peak memory, startup и Stop→ready.
  Источник 1 — настоящий нулевой PCM; это не аппаратная двухчасовая запись.

## Исторические gates и границы

Исходный R4 сравнивал нынешний early capture с прежним offline без ledger/planner.
Его +48.12%, последующий resident legacy fail (+18.51%, две пары >20%) и успешный
дополнительный current-capture контроль +2.95% сохраняются со своим статусом.
Они не переписываются и не объявляются новым успешным прогоном. PIPE-019 gate
принятия оптимизации не меняется. Здесь принимается отдельное явное решение о
default существующего полного пути по новой полной проверке текущего переключателя.

Если полный current-capture контроль не проходит, default пока не меняется:
сначала исследуется причина без ослабления порога, повторные попытки сохраняются.
После pass: .env.defaults=on, отсутствие override включает режим, явный off
сохраняется, UI/TUI видят on и позволяют сохранить off/on/reset без перезапуска.
Проверяются Rust/API/browser/ConPTY и real coordinator Stop/disable/cancel/retry.
После этих проверок собирается согласованный bundle с новым baked preset.
7.4 microphone/loopback/Mac остаётся not-run; sync/archive/push не выполняются.
