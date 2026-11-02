# Измерение стоимости worker-фаз

`localvox-profile` измеряет последовательные отдельные процессы распознавания и
опциональной summary на копиях WAV. Это инженерный инструмент для BL-003/008.
Полный прогон координатора и двухчасовой встречи учитывается отдельно.

Для готового транскрипта есть [отдельный text-only режим](text-postprocessing-profile.md):
он сравнивает refine, cleanup, их последовательность и summary без повторного ASR.

Соберите текущие исходники, не заменяя установленный комплект приложения:

```powershell
$env:TEMP = "$PWD\target\test-temp"
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null
cargo build --locked --release -j 2 -p localvox-light-asr --bin localvox-process -p localvox-light-bench --bin localvox-profile
```

Для локально загруженного набора Golos (`bench/golos-crowd/audio/00000.wav`–`00004.wav`)
и GigaAM из `models/gigaam-v3-e2e-ctc`:

```powershell
.\target\release\localvox-profile.exe --manifest scripts/fixtures/profile-golos-five.json --worker target/release/localvox-process.exe --model-dir models/gigaam-v3-e2e-ctc --output target/profile-golos-five-run1 --repeats 2 --timeout-sec 120
```

Путь `--output` должен быть новым: повтор выбирает другое имя каталога. Инструмент
не перезаписывает и не удаляет прошлые результаты. Каждый case/repeat получает
новую сессию с копией аудио; исходник не меняется. Пользовательские `.env`, archive,
настройки демона и облачный провайдер не используются. На каждой фазе действует
timeout; `Ctrl+C` кооперативно отменяет принадлежащее процессное дерево.

Другой manifest:

```json
{
  "version": 1,
  "label": "Описание тестового набора и условий",
  "cases": [
    { "name": "meeting", "audio": "relative/to/manifest.wav", "summary": true }
  ]
}
```

Нужен непустой PCM16 mono 16 kHz WAV. Имена cases — латинские буквы, цифры, `_`, `-`;
они уникальны. `summary` по умолчанию выключена. Для неё явно задаются
`--llm-base-url http://127.0.0.1:11434/v1` и `--llm-model ...`; поддерживаются только
локальные HTTP endpoints. Сервер/модель должны быть подготовлены отдельно;
неявного перехода на Claude/облачный API нет. Диаризация и автоматические
refine/cleanup в этом профиле выключены, глоссарий пустой.

Для раздельного измерения summary добавьте `--trace-ollama`. Временный loopback
proxy передаст native JSON без изменений и сохранит только счётчики, options и
численные timings ответа. Принимается только native `/api/chat`; redirects не
передаются. Внутренний адрес содержит `/:11434/v1` для существующего определения
диалекта worker. Пользовательский upstream остаётся указанным `--llm-base-url`.

NER по умолчанию берётся из `ner-gliner` рядом с ASR model-dir; другой путь задаётся
`--ner-model-dir`. Выбор фиксируется в окружении child, файлы входят в fingerprint.
Отсутствие модели отмечается явно: наличие файла отдельно от успешной загрузки
(`ner_loaded_worker_diagnostic`). Это исправляет измерительный профиль, приложение
и его поиск моделей не меняются.

В `<output>/report.json` после каждого исхода сохраняется checkpoint:

| Поле | Что означает |
|---|---|
| `inputs`, fingerprints | Длительность WAV, SHA-256 исходников, runner/worker, manifest и файлов модели; конфигурация и порядок repeats |
| `preflight_sec`, `batch_wall_sec`, `total_wall_sec` | Предварительное чтение/fingerprint и проверка входов; выполнение cases; весь запуск до финального checkpoint |
| `setup_sec` | Создание изолированной сессии и копирование аудио; не время распознавания |
| `phases[].wall_sec` | Время запуска и завершения отдельного worker с надзором |
| `acceptance_verification_sec` | Отдельная проверка сохранённого артефакта измерителем после выхода worker |
| `resources` | Windows: наблюдаемый OS high-water mark памяти и CPU **самого worker**; опрос каждые 20 ms и финальное чтение retained handle |
| `worker_wall_sec`, `rtf` | Сумма успешных worker-фаз и отношение к длительности WAV; failed/timeout/cancel не становятся быстрым успехом |
| `timeline.stages/operations` | Записанные границы этапов и длительности операций; вложенные операции не складываются в общее время |
| `queue_wait_sec`, model-load/generation/verification intervals, external LLM resources | `null`, если границ/измерений нет; ноль не подставляется |
| `complete`, `all_success` | Все запланированные cases обработаны / все завершились с проверенным артефактом |
| `phases[].llm_trace` | Число HTTP-запросов, load/prompt/generation по Ollama, параметры контекста/лимита и отдельные запросы; поля без stats остаются null |
| `non_http_worker_wall_sec` | Worker wall минус сумма HTTP wall: загрузка/проверки/прочая работа вместе, без утверждения, что всё это NER |
| `content_unverified` | У результата остались сомнения проверки содержания; `ready` подтверждает сохранность артефакта |

Checkpoint сохраняется после каждого исхода фазы: во время summary уже доступен
принятый STT-замер с `run.complete=false`; частичный case не имеет успешного RTF.
В trace mode отчёт не содержит messages, ответы, headers, свободный диагностический
текст или error notes. Операции имеют фиксированные категории с сохранёнными
временами. Нормальные артефакты тестовой сессии (транскрипт/summary) сохраняются
обычным worker и не являются частью телеметрии.

Proxy не владеет внешним Ollama: отмена local worker не доказывает остановку
вычисления сервера. Остановка proxy не блокируется на upstream; незавершённый запрос
остаётся `pending`, длительность генерации неизвестна. `done_reason` сохраняется
только для известных значений stop/length/load/unload; старые отчёты без этого поля
не доказывают причину завершения.

Ресурсы ffmpeg/других потомков и отдельного Ollama этим значением памяти не
измеряются. На других ОС Windows-метрики недоступны. Выборка OS high-water mark
может не увидеть пик очень короткого процесса; отсутствие query — явный `null`.
Идентификаторы входов читаются для SHA-256 до запуска: первый процесс нельзя
называть замером холодного дискового cache. Каждый повтор заново загружает ASR
в новом процессе, состояние внешнего LLM/cache ОС не контролируется автоматически.

Короткая контрольная пачка проверяет воспроизводимость инструмента и стоимость
запуска, но не задаёт бюджет длинной записи. Для полного BL-003 нужны длинная речь,
настоящие модели summary/проверок, координатор/очередь и документированные условия
машины. Затем можно выбирать оптимизацию загрузки моделей, памяти или обработки.

Проверки без speech-моделей:

```powershell
cargo test --locked -j 2 -p localvox-light-bench --bin localvox-profile -- --test-threads=2
```

## Настоящий координатор без захвата звука

`localvox-queue-profile` запускает библиотечный `autocook` приложения без main,
микрофона, HTTP API и UI. Политика фаз, линии ресурсов, retries, проверка артефактов
и сохранение очереди общие с daemon. Сам импорт библиотеки ничего не запускает.

```powershell
cargo build --locked -j 2 -p localvox-light-asr --bin localvox-process -p localvox-light-bench --bin localvox-queue-profile
target/debug/localvox-queue-profile.exe --work-dir target/profile-new --worker target/debug/localvox-process.exe --timeout-sec 900
```

Это вход для изолированного стенда: внешний runner заранее копирует fixtures,
создаёт `sessions`/`jobs.json` и передаёт явное окружение. `.env` не читается.
Обязателен маркер `.localvox-profile-workspace` с содержимым `localvox-profile/v1`
и переводом строки; `localvox-profile` создаёт его только в новом output.
Нельзя запускать инструмент на пользовательском архиве. Активная запись,
нечитаемая очередь, небезопасный путь сессии, Running job, повторный отчёт и занятый
профиль отклоняются. Worker должен совпасть с sibling, который найдёт координатор.
Профили разных сборок не смешиваются.

`queue-report.pending.json` — неполный checkpoint раз в секунду;
`queue-report.json` публикуется после кооперативной остановки и join. Пустая очередь
успешна только после настоящего protocol preflight и ready координатора. Failed,
таймаут, Ctrl+C, ошибка очереди/координатора и потеря событий не становятся успешным
замером. Отчёт хранит metadata jobs и события `claimed`/`accepted`/`failed`; текст
встреч и `last_error` в него не входят. `complete` означает терминальный исход без
потерь, `all_success` — все задания Done. Никаких выводов о качестве текста.

Observer отправляет события `try_send` после сохранённого перехода под mutex очереди.
Он не ждёт потребителя; overflow фиксируется `events_lost`. Часы взяты из Job,
`queue_wait_sec` и `phase_wall_sec` — их разности. Неизвестные, повреждённые и
обратные границы дают null. Время фазы включает проверку и принятие артефакта;
это не отдельное время модели. Память/GPU и внешняя LLM здесь ещё не измеряются.

Model-free runtime проверяет пустую очередь, terminal Failed после трёх попыток,
timeout/stop с сохранённым заданием и несовместимый worker. Сначала нужна сборка
двух binaries из команды выше:

```powershell
cargo test --locked -j 2 -p localvox-light --lib --bin localvox-light -- --test-threads=2
cargo test --locked -j 2 -p localvox-light-bench --bin localvox-queue-profile -- --test-threads=2
cargo test --locked -j 2 -p localvox-light-bench --bin localvox-queue-profile -- --ignored --exact tests::real_coordinator_empty_timeout_and_preflight_failure_are_bounded
```

Coordinator tests перенесены из binary в lib без копии реализации; launcher tests
остались в binary. CI запускает оба. Создание стенда не закрывает полный BL-003
или аппаратную приёмку записи/плеера.

## Пачка из manifest через координатор

Добавьте `--queue-runner` из той же сборочной папки, что и `--worker`:

```powershell
cargo build --locked --release -j 2 -p localvox-light-asr --bin localvox-process -p localvox-light-bench --bin localvox-profile --bin localvox-queue-profile
target/release/localvox-profile.exe --manifest scripts/fixtures/profile-golos-five.json --worker target/release/localvox-process.exe --model-dir models/gigaam-v3-e2e-ctc --queue-runner target/release/localvox-queue-profile.exe --output target/profile-queue-new --repeats 2 --timeout-sec 900
```

Весь manifest/repeats сначала копируется в новые sessions. Подготовленное PCM
получает source metadata и проверенный preparation receipt; затем публикуются
настоящие Cook jobs. Prepare принимает готовое аудио обычным путём. Загрузка из
сети и конверсия здесь исключены. В одной пачке `summary` одинакова для всех cases;
для сравнения переключателя нужны два manifest. Automatic cleanup/refine и
диаризация выключены. Interval координатора — обычные 5 s, ресурсные линии прежние.

`--timeout-sec` в этом режиме ограничивает всю очередь и каждую фазу; внешний
supervisor даёт 15 s на её кооперативное завершение. Ctrl+C внешнего профилировщика
останавливает принадлежащее дерево, поэтому финальный queue report может остаться
pending. Сохранённые исходы и артефакты не превращаются в успешный неполный замер.

`report.json` содержит `queue` с точными переходами, проверенные состояния
артефактов/timelines каждой сессии и `coordinator_process.resources` с наблюдаемыми
потомками на Windows. Суммарный пик текущей памяти считается по одновременной
выборке каждые 100 ms, индивидуальные OS peaks хранятся отдельно; они не
складываются в «пик дерева». Короткие процессы могут быть пропущены. Внешний
Ollama в эти ресурсы не входит. Per-job `rtf` остаётся null: фаза координатора
включает проверку/commit, отдельный worker wall здесь не замерен. Фазы разных
сессий пересекаются; отношение batch wall к сумме audio_sec — throughput пачки.

Только профилировщик задаёт `LOCALVOX_PROFILE_TIMINGS=1`: операция `asr_model_load`
измеряет создание adapter/vocabulary и inference session внутри STT. Обычное
приложение эту операцию не добавляет. Она вложена в STT и не прибавляется к нему.
`--trace-ollama` сохраняет общий trace с started/ended timestamps запросов; при
параллельных фазах отдельная сессия не назначается HTTP-запросу автоматически.
Raw labels/notes/errors в queue-отчёте отсутствуют. Hash worker и queue runner
повторно проверяется после завершения; пересборка во время запуска делает замер
неполным.

Для отдельного внешнего замера на Windows/PowerShell 7.4+ есть
[рецепт](../openspec/changes/benchmark-processing-batch/evidence/profile-external.ps1):
запустите из корня с `-Manifest ... -Output target/profile-new -Repeats 2
-TimeoutSec 1800 -TraceOllama`. Он использует существующий локальный Ollama,
снимает API model residency, observed CPU/RAM его процессов и **глобальную**
NVIDIA GPU-память/загрузку примерно раз в 2 s. Внешний сервер не останавливается.
GPU включает другие приложения; `size_vram` означает выделение модели. Ресурсы
и результаты профилировщика сохраняются в отдельных связанных JSON.
