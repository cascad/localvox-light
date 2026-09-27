# Runtime and settings Specification

## Purpose

F3/F6/P7. Жизненный цикл демона/окна, сборка, настройки и автостарт.

Статус контракта: **accepted**, 2026-09-27. Это обязательное поведение,
а не заявление о безошибочности текущей сборки.
[Основание принятия и границы](../../baseline.md) ·
[Сверка каждого требования](../../evidence/coverage.md#runtime-settings).

## Requirements

### Requirement: RUN-001 Окно отдельно от движка

Tauri SHALL проверять TCP-доступность адреса, при отсутствии слушателя пытаться запустить соседний localvox-light --daemon и ждать до 15 с. При неуспехе показывает страницу «Движок не отвечает». Повторное окно фокусирует main через single-instance plugin. Закрытие окна не содержит команды завершить демон.

Код: [ensure_daemon, listening, run](../../../src-tauri/src/lib.rs).

#### Scenario: Движок не отвечает

- **WHEN** соседний демон не начал слушать за 15 секунд
- **THEN** показывается диагностическая страница.

#### Scenario: Второй запуск окна

- **WHEN** main уже существует
- **THEN** оно поднимается и фокусируется.

### Requirement: RUN-002 Комплект бинарников и UI

Демон SHALL искать localvox-process рядом со своим exe и проверять worker-протокол перед захватом очереди. Frontend берётся rust-embed из ui/dist: release содержит сборочный снимок, debug может читать каталог. Сборка только пакета демона не является сборкой workspace/worker. Windows scripts/run.ps1 собирает демон и worker одним вызовом Cargo в выбранном профиле (release по умолчанию, dev по запросу), при Desktop добавляет окно; BuildOnly не запускает приложение. Пути бинарников берутся из сообщений Cargo, cwd — корень репозитория.

Код: [Config::from_env, protocol preflight](../../../crates/localvox-light/src/autocook.rs), [WebApp](../../../crates/localvox-light-api/src/http.rs), [workspace](../../../Cargo.toml), [запуск из исходников](../../../scripts/run.ps1).

После миграции: [проверка сборки и протокола 2026-09-27](../../evidence/worker-startup.md).

#### Scenario: Несовместимый worker

- **WHEN** протокол соседнего обработчика отличается
- **THEN** координатор не начинает выполнять задания им; лог содержит путь worker, ожидаемый/полученный протокол, результат запуска и команду исправления. После обновления комплекта требуется перезапуск демона; preflight автоматически не повторяется.

#### Scenario: Ошибка сборки перед запуском

- **WHEN** Cargo завершился с ошибкой при вызове scripts/run.ps1
- **THEN** скрипт завершается ошибкой, старые бинарники не запускаются.

#### Scenario: Пересборка фронта release

- **WHEN** изменён ui/dist без пересборки демона
- **THEN** уже собранный release бинарник не заменяет встроенный frontend.

### Requirement: CFG-001 Сохранение настроек

POST settings SHALL разрешать ключи LOCALVOX_* и RUST_LOG, преобразовывать bool в on/off, number в строку, null/пустую строку в unset. env_file сохраняет изменения с остальными строками/комментариями, затем apply_live обновляет процессное окружение. Ошибка сохранения предшествует применению.

Код: [POST /api/settings](../../../crates/localvox-light-api/src/http.rs), [save, apply](../../../crates/localvox-light-core/src/env_file.rs), [writable, apply_live](../../../crates/localvox-light-core/src/settings.rs).

#### Scenario: Сброс

- **WHEN** значение передано null
- **THEN** ключ снимается через env_file и переменную процесса.

#### Scenario: Запрещённый ключ

- **WHEN** передан ключ вне разрешённых префиксов
- **THEN** изменение отклоняется.

### Requirement: CFG-002 Область действия настроек

Каталог настроек SHALL различать Live/Capture/Restart по фактическому потребителю. Live означает применение к следующим операциям без перезапуска; если потребитель держит snapshot, API/UI SHALL сообщать необходимость перезапуска, а не обещать Live. Capture вызывает reload_capture_from_env; при отсутствии зарегистрированных controls сообщает restart. Уже выполняющаяся операция может завершиться со своим снимком настроек.

Код: [applies, apply_live](../../../crates/localvox-light-core/src/settings.rs), [Config::from_env, child_env](../../../crates/localvox-light/src/autocook.rs).

#### Scenario: Смена LLM после старта

- **WHEN** настройка сохранена, но autocook уже держит свой snapshot
- **THEN** настройка помечается требующей restart либо snapshot обновляется для следующего job; одного set_var недостаточно для отчёта Live.

#### Scenario: Нет capture controls

- **WHEN** изменён микрофон через отдельный API-процесс
- **THEN** сообщается необходимость перезапуска.

### Requirement: CFG-003 Показ секретов и значений

GET settings SHALL для известных secret-полей каталога возвращать value=null и признак set; незаданные значения не подменяются числом из hint. Неизвестные ключи .env SHALL не публиковать исходные значения через extra: неизвестность не доказывает отсутствие секрета. Для них допустимы имя и признак наличия, без значения; раскрытие нового обычного поля требует явной классификации в каталоге.

Код: [GET /api/settings](../../../crates/localvox-light-api/src/http.rs), [CATALOGUE, Setting::secret](../../../crates/localvox-light-core/src/settings.rs).

#### Scenario: Известный secret

- **WHEN** поле отмечено Kind::Secret в каталоге
- **THEN** ответ сообщает наличие, не значение.

#### Scenario: Неизвестный ключ

- **WHEN** ключ отсутствует в каталоге
- **THEN** его исходное значение не возвращается; поле не обходится через extra.

### Requirement: RUN-003 Windows автостарт

Windows autostart SHALL сохранять HKCU Run для текущего exe с --daemon и --cwd; сравнивать путь exe без учёта регистра, а не всю командную строку. После перемещения бинарника старый путь не считается включённым автозапуском. Этот модуль cfg(windows); LaunchAgent отсутствует.

Код: [run_command, is_enabled, enable, disable](../../../crates/localvox-light-core/src/autostart.rs).

#### Scenario: Бинарник перемещён

- **WHEN** Run указывает другой путь
- **THEN** статус автостарта текущего exe выключен.

### Requirement: INV-RUN-001 Component shutdown diagnostics

Остановка демона SHALL устанавливать сигнал отмены до ожидания компонентов.
Ожидание SHALL сообщать имя компонента и прошедшее время; превышение лимита
или panic SHALL не сообщаться как успешный штатный выход. Ошибка остановки одного
компонента SHALL не пропускать попытку сохранить и завершить остальные.

#### Scenario: Worker coordinator exceeds shutdown limit

- **WHEN** autocook не завершился за отведённое время
- **THEN** ошибка явно называет autocook, а завершение engine всё равно запрашивается и проверяется.

#### Scenario: Engine thread panics

- **WHEN** ожидаемый поток engine завершился panic
- **THEN** это диагностируется как ошибка, не обычное завершение.

Код: [engine](../../../crates/localvox-light-core/src/engine.rs), [pipeline](../../../crates/localvox-light-core/src/pipeline.rs), [named join](../../../crates/localvox-light-core/src/cli.rs), [daemon](../../../crates/localvox-light/src/main.rs).

## Implementation status

CFG-002 неверно обещает Live при snapshot (RV-001); CFG-003 возвращает неизвестные значения через extra (RV-010). INV-RUN-001 проверен; исходная причина 10-секундного зависания остаётся BL-018. Desktop bundle собран без перезапуска пользовательского приложения.

[Матрица по требованиям](../../evidence/coverage.md#runtime-settings) ·
[Расхождения RV](../../review.md) ·
[Выполненные проверки](../../evidence/implementation-review-2026-09-27.md).
Ниже сохранены дополнительные наблюдения о реализации, не исключения из требований.

## Observed limitations

RV-001: рассогласование каталога Live и snapshot autocook; RV-010: extra не фильтрует неизвестные секреты. TCP-проверка окна не проверяет, что порт занят именно LocalVox. Живые macOS/Linux, tray и аппаратный capture при миграции не проверены; старые слова «должно работать везде» не подтверждение. Restart окна не равен restart демона.

RV-014 / [BL-018](../../backlog.md#bl-018): исходный общий лог 10 с не позволяет
определить зависший компонент. В текущих исходниках [Daemon::shutdown](../../../crates/localvox-light/src/main.rs)
сначала устанавливает running=false, затем отдельно проверяет autocook/engine;
[named join](../../../crates/localvox-light-core/src/cli.rs) пишет компонент/время
и возвращает ошибку timeout/panic. Проверены отмена ожидания модели и управление
без PCM. Причина конкретного release-инцидента ещё не доказана; требования/проверки
продолжаются в change. [Evidence](../../evidence/implementation-review-2026-09-27.md).
