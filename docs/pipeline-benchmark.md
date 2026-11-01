# Измерение стоимости worker-фаз

`localvox-profile` измеряет последовательные отдельные процессы распознавания и
опциональной summary на копиях WAV. Это инженерный инструмент для BL-003/008.
Полный прогон координатора и двухчасовой встречи учитывается отдельно.

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
