# Выбираемый ASR и независимая пунктуация · 2026-11-24

Изменения реализованы в исходниках. Это отдельный отчёт от
[проверки нативных моделей](native-models.md); опубликованный релиз и проверка
аппаратного звука этим не подтверждаются. Delta пока не sync/archive.

## Поведение

- `LOCALVOX_ASR_BACKEND=gigaam|tone`, `LOCALVOX_ASR_PUNCTUATION=off|on` в общих
  предустановках и каталоге UI/TUI. Default — GigaAM без Sbert. Новая сессия хранит
  выбор в meta; legacy не следует за новым default. Переварка принимает новый
  выбор после проверки готовности, выполняющаяся запись не переключается.
- `ModelPackage` и `Selection` связывают раннюю/финальную ASR с одним engine,
  recipe и dependency identity. T-one имеет отдельный raw PCM/state адаптер.
- Пунктуация — отдельные `TextProcessor`, cache и `JobPhase::Punctuation` /
  `Stage::Punctuation`, не LLM cleanup. Финальный текст — новая версия с parent,
  identity, checksum и receipt `punctuated`; raw не переписывается.
  Числа, слова, источник, участник и времена проверяются перед публикацией.
- Ранний snapshot хранит raw и identity оформления. При отказе публикуется
  проверенный raw, операция возвращает ошибку, акустический checkpoint остаётся.
  Повтор/Stop используют уже принятые окна. Кеш пунктуации переиспользуется
  отдельно, испорченные cache/version не подтверждаются повторно как результат модели.
- Setup/`vox setup`: активный GigaAM/T-one, отдельные галочки запасного ASR и Sbert,
  готовые пути или общий downloader. Невыбранная отсутствующая модель не блокирует
  рабочий стек. Doctor делает только проверки наличия файлов, не inference.
- Worker protocol — `localvox-worker/4`, publication preflight проверяет ту же
  версию. Установленный runtime не требует Python, Triton или NVIDIA.
- Пользовательские действия: [US-059/068/069](../../../../docs/user-guide.md).

## Фактические проверки Windows

Все модельные/тяжёлые группы выполнялись последовательно, Cargo jobs 1,
Rust test threads 1; ASR/Sbert runtime не более 2 CPU threads.
Рабочий демон, архив и устройства пользователя не использовались.

- Core: **513 passed, 13 ignored** — включая durable phase/retry, provenance,
  raw fallback, corrupt text cache/version, legacy choice, doctor, long-path atomic write.
- API: **97 passed, 8 ignored**; TUI: **252 passed, 3 ignored**. Включены сохранение
  выбора, restart scope, отказ смены backend до изменения результатов при отсутствии
  T-one/Sbert, явная смена после готовности, общий контрол выбора и подтверждение `s`.
- Настоящий worker без весов: **8 passed**, включая pinned T-one recipe,
  crash/restart, missing Sbert → raw publication, retry/Stop без повторного ASR.
- Installer: **13 passed**, setup **11 passed**, native preview ConPTY **13 passed**:
  T-one-only, оба ASR, независимый Sbert с GigaAM, готовые внешние пути,
  повтор без actions/changes, галочки/возврат/отмена.
- Настоящий worker с pinned T-one + Sbert: **1 passed, 24.88 s** на публичном
  коротком WAV, отдельно off/on; исходная версия и времена производной проверены.
  Это не benchmark двухчасовой записи. Лог: `target/review/tone/real-worker-2.log`.

В первом сквозном model run воспроизведён отказ Windows rename при пути кеша
длиннее 260 символов. Исправлен общий atomic writer: существующий родитель
канонизируется в Windows verbatim path для обоих имён; копирование вместо rename
не добавлялось. Повтор на настоящих весах и отдельный long-path regression прошли.

Изменён один визуальный эталон после просмотра diff/actual: общий заголовок
распознавания вместо жёсткого GigaAM. Функциональные browser assertions не ослаблялись.

## Итоговый повтор

- Полный runner: `target/behavior/20261124T154536-12848`, 18 шагов.
  В нём прошли TUI/PTY, core, worker, API/settings, installer, delivery и браузер;
  два отказа (mpv и cross-client fixture) сохранены в исходном отчёте как failed.
- После исправления повторены все затронутые `recording/api/mpv/gui`:
  `target/behavior/20261124T155257-18380` — **12/12 шагов passed**.
  Core **514 passed / 13 ignored**, worker **8 passed**, archive API **34 passed / 2 ignored**,
  settings HTTP **1 passed**, journal HTTP **1 passed**, real mpv **1 passed**,
  настоящий API → GUI/TUI **1 passed**, browser **88 passed**, StrictMode **6 passed**.
  Для браузера включено строгое сравнение изображений, обновление эталонов выключено.
- Delivery: **85 tests, 65 passed / 20 skipped**, в том числе реальные PS/Bash
  localhost загрузки optional tone/sbert, SHA-256 и повтор без новых HTTP запросов.
  Installer/root library/command tests: **50 passed / 12 ignored**;
  preview library + ConPTY: **14 passed**. Неизменённые прошедшие группы не маскируются повтором.
- Cross-client fixture больше не создаёт фиктивный Done для невыбранной пунктуации.
  Во всех null-audio mpv seek assertions используется одинаковая граница 250 ms
  для наблюдаемого `time-pos`, учитывающего выходной буфер; pause, точная команда seek
  и точный endpoint фрагмента проверяются по-прежнему. В первом отказе наблюдалось
  1849.830852 вместо цели 1850, что укладывается в уже применявшуюся границу другого seek.
- После последней проверки отказа очереди отдельно повторён тест API транзакции:
  **1 passed** (`target/review/tone/recook-transaction.log`). Он проверяет сохранность
  meta/raw/summary при отказе, а также смену плана уже существующего Done job.
- CLI explicit force с недоступной заменой: **1 passed** (`cli-choice.log`), прежний
  выбор и raw остаются неизменными. Новый план пунктуации сохраняется уже при enqueue.
- Финальный native model worker: **1 passed, 24.57 s** (`real-worker-final.log`);
  это повтор после добавления проверки checksum производной версии.
- Полный финальный TUI: **252 passed / 3 ignored** (`tui-final.log`), в том числе
  видимость новых переключателей в базовом списке рядом с уже существующими настройками.
- Финальные проверки после последнего изменения discovery/очереди: **59 jobs tests passed**,
  runner self-tests **9 passed** (`jobs-final.log`, `runner-tests.log`). Для ASR-only
  сессий discovery не читает тексты, если производные не запрошены.
- Strict main specs: **15 passed**; change strict — valid; check-openspec:
  **15 capabilities / 88 main requirements / 188 scenarios**, links pass; diff check pass.

## Границы

Mac/Linux native, микрофон/loopback, WER на общем с GigaAM корпусе и длительный
production benchmark — **not-run**. Greedy не приравнивается к beam/KenLM.
GigaAM default не менялся; Sbert не включается установкой второго ASR автоматически.
