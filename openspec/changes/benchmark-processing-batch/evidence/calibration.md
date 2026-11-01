# Контрольные worker-замеры · 2026-11-01

BL-003/008, PIPE-004/008/009/011. Инструмент реализован, контрольные release-прогоны
выполнены. Полный критерий BL-003, бюджет и аппаратная приёмка остаются открытыми.

## Что измерено

[localvox-profile](../../../../crates/localvox-light-bench/src/bin/localvox-profile.rs)
запускает последовательные fresh-process worker-фазы на копиях WAV через общий
процессный supervisor. Окружение очищено, пользовательский dotenv не читается,
TEMP/TMP находятся внутри нового output на D:. Диаризация/refine/cleanup выключены,
summary в этих двух контрольных наборах не запускалась. Живой демон, архив и
установленный комплект не изменены. Собран текущий target/release worker.

Настройка и команды: [руководство](../../../../docs/pipeline-benchmark.md).
Имена файлов модели в отчёте — строки; артефакт после выхода проверяется независимо
от exit code, его ready/empty явно сохранён. Источники не меняются, существующий
output не перезаписывается. При failed/timeout/cancel RTF успеха не вычисляется.

Сводка относится к Windows x86_64. Win32/CIM сообщили AMD Ryzen 9 9900X3D,
6 доступных логических CPU, 66 157 793 280 байт физической памяти (~61.6 GiB).
Фоновая нагрузка машины не контролировалась, профиль CPU/GPU независимо не измерялся.
Нельзя переносить числа на другое оборудование или считать их бюджетом продукта.

## Итоговые данные

Полные JSON с fingerprint входов/worker/runner/manifest/моделей и временами:
[пять коротких WAV](golos-five-2026-11-01.json), [длинный WAV](longform-2026-11-01.json).
Это итоговые отчёты с совпадающим SHA-256 текущего release runner:

```text
runner 7961bb51e0be46abc4874dc565a942a1660caf5f9305a0eabdfd99f050e1b43b
worker 9ad460f6922ff9e86c69a129fbd844885840519fa5637626b880c0ad1e0699d2
GigaAM int8 2e3fcb7a7b66030336fd10c2fcfb033bd1dc7e1bf238fe5cfd83b1d0cfc9d28e
```

Первый набор — первые пять PCM WAV локального публичного Golos corpus,
[manifest](../../../../scripts/fixtures/profile-golos-five.json). В каждом результате
одна непустая строка транскрипта и проверенный ready-артефакт, а не быстрый empty.

| Case | Аудио, s | Worker r1, s | Worker r2, s |
|---|---:|---:|---:|
| golos-00 | 4.00 | 4.1672 | 2.7082 |
| golos-01 | 3.43 | 1.0164 | 0.9970 |
| golos-02 | 3.06 | 1.3431 | 0.8842 |
| golos-03 | 5.44 | 1.8702 | 1.0992 |
| golos-04 | 5.22 | 2.0874 | 1.4354 |

Всего 10/10 успешно: 42.30 s аудио, 17.6082 s суммы worker-фаз, batch 17.7410 s,
preflight 0.4916 s, total 18.2356 s. Максимальный наблюдаемый worker working set
275.74 MiB. Разброс r1/r2 не доказывает эффект прогрева: cache и фоновые процессы
не контролировались. Хеширование предварительно прочитало модель/входы.

Дополнительный публичный `bench/lf-dense/lf_001.wav`: 602.1295 s аудио,
**7.8271 s worker**, RTF 0.0130, **314.17 MiB** наблюдаемого peak working set,
2.8125 s CPU counter worker. Сохранены 59 непустых строк и ready-артефакт;
batch 7.8511 s, preflight 0.4140 s, total 8.2687 s. Лимит был 120 s, изменять его
для успеха не потребовалось. Это один direct-worker запуск, не замер всей очереди.
Качество текста/WER в этом прогоне не измерялось.

## Воспроизводимость и существенные проверки

- Manifest/аудио: непустой PCM16 mono 16 kHz, уникальные безопасные case names,
  отказ отсутствующего файла/неверного формата, source checksum остаётся прежним.
- Existing output с sentinel отклоняется без изменений. Нелокальные/неоднозначные
  endpoints отклоняются; worker получает явный local provider и новое окружение.
- Настоящие subprocess tests: nonzero exit, deadline, cancellation checkpoint;
  failed не попадает в успешный RTF. Копия аудио тождественна источнику,
  SessionMeta читается штатным типом и сохраняет явный язык/закрытие.
- Native Windows child выделяет 32 MiB; после завершения остаются наблюдаемый peak,
  CPU counter и worker-only scope. Краткий процесс может законно иметь CPU=0:
  первый повторный test-run обнаружил, что observer отбрасывал этот известный ноль
  как unknown (`target/batch-tests-first-failure.log`: 6 pass, 1 fail, 1 ignored).
  Observer теперь сохраняет полученный CPU=0; null остаётся отсутствием измерения.
- Timeline test сохраняет 3 s границ с повторным heartbeat, отдельные вложенные
  операции, malformed/incomplete tail и missing history; неизвестная загрузка
  модели/генерация не превращаются в ноль или сумму вложенных длительностей.

Первый контрольный запуск `target/profile-golos-five-20261101-a` пересёкся с
незавершённой пересборкой runner и использовал промежуточный формат. Следующий
`...-b` также предшествовал финальной правке имён файлов/fingerprint.
Оба сохранены как промежуточные, в итоговые выводы не включены. Добавлены SHA самого
runner и manifest, итоговый `...-c` запущен после завершения сборки и проверен по SHA.
JSON выше сохранены без текстов транскриптов и секретных значений окружения.

| Команда | Результат |
|---|---|
| `cargo test --locked -j 2 -p localvox-light-bench --bin localvox-profile -- --test-threads=2` | PASS: 7 passed, 1 ignored subprocess fixture; она запускается явно родительскими тестами |
| `cargo build --locked --release -j 2 -p localvox-light-asr --bin localvox-process -p localvox-light-bench --bin localvox-profile` и финальная пересборка runner | PASS; прежние 2 core warnings |
| `localvox-profile --manifest scripts/fixtures/profile-golos-five.json ... --repeats 2 --timeout-sec 120` | PASS: 10/10, source/runner/worker fingerprints и ready receipts проверены |
| `localvox-profile --manifest target/profile-longform-one.json ... --repeats 1 --timeout-sec 120` | PASS: 1/1, 602.1295 s публичной речи; manifest fingerprint внутри отчёта |
| `openspec validate --specs --strict --no-interactive`, CLI 1.13.2 | PASS: 15/15; прежние INFO о длине требований |
| `openspec validate benchmark-processing-batch --type change --strict --no-interactive` | PASS: skip_specs подтверждён, runtime-поведение не менялось |
| `python scripts/check-openspec.py` | PASS: 15 capability, 86 main требований, 168 сценариев, 86 coverage rows |
| `rustfmt --edition 2021 --check crates/localvox-light-bench/src/bin/localvox-profile.rs`, `git diff --check` | PASS |
| Сверка JSON отчётов | PASS: SHA текущих runner/worker совпадают, all_success/ready, RTF соответствует wall/audio, неизвестные измерения null |

Полные команды/логи — `target/batch-{release-build,runner-build-final,tests,golos-final,longform}.log`;
новый CLI описан в docs, tests и строгая проверка change добавлены в interface CI.
Строгие main/change/check-openspec/format/diff выполнены.
Удалённый CI не запускался. Производственный код и UI/TUI не менялись,
пользовательский справочник US-001–046 сохраняет прежнее поведение.

## Что ещё требуется для BL-003

Задачи 2.2/2.3 не выполнены: координатор/queue wait, двухчасовая репрезентативная
речь, настоящая summary, отдельные model-load/generation/verification границы,
контролируемые cold/warm условия и внешние ресурсы. Прямые worker-числа не позволяют
объяснить прежний час постобработки или обещать бюджет. Готовое средство позволяет
воспроизводимо продолжить эти измерения. Вопрос постоянного STT-worker остаётся
исследованием BL-008; выбранной новой архитектуры этот срез не объявляет.
