## MODIFIED Requirements

### Requirement: SETUP-002 Разрешение зависимостей

Штатные бинарники с Vosk SHALL использовать нативную библиотеку, требуемую загрузчиком ещё до main. Расположение ASR-модели выбирается из явного CLI/env пути, существующего соседнего models либо cwd, затем пользовательского каталога по SETUP-008; worker ищется рядом с daemon. Указание модели в другом месте не заставляет doctor проверять лишь установочный каталог.

Код: [model paths](../../../../../crates/localvox-light-core/src/model_paths.rs), [native linking](../../../../../crates/localvox-light-core/build.rs), [default_model_dir](../../../../../crates/localvox-light-core/src/lang.rs), [Layout::from_env](../../../../../crates/localvox-light-core/src/doctor.rs).

#### Scenario: Модель задана явно

- **WHEN** существующий путь отличается от bundled models
- **THEN** диагностика использует путь разрешения модели.

## ADDED Requirements

### Requirement: SETUP-008 Пользовательский каталог моделей

Без явного пути и существующего portable/CWD расположения LocalVox SHALL использовать
`~/.localvox/models` на Linux/macOS и `%USERPROFILE%\.localvox\models` на Windows.
Имя `.localvox` SHALL быть строчным. Явные CLI и индивидуальные env пути моделей
SHALL иметь приоритет, включая путь на другом диске; нерабочий явный путь MUST NOT
незаметно заменяться другим. Уже существующие модели SHALL переиспользоваться без
автоматического переноса. Хранилище Ollama этим правилом не изменяется.

Runtime, doctor, мастер и штатная загрузка SHALL разрешать одинаковые пути.
Установленный CLI SHALL предоставлять read-only JSON разрешённых путей и их
источников без запуска inference, записи, сетевых запросов или создания каталогов.
Check SHALL оставаться read-only. Недоступный home без подходящего explicit/portable
пути SHALL быть понятной ошибкой загрузки, а не поводом писать в cwd.
Standalone загрузчик при наличии `.env` и отсутствии native resolver SHALL
требовать явный Root вместо частичного/небезопасного чтения конфигурации.

Новая установка SHALL не записывать устаревшие install-relative model overrides.
Явный fetch `--root/-Root` SHALL сохранять прежнюю portable семантику
`<root>/models`. Загрузчик SHALL использовать манифест и переиспользовать готовое;
неисправный configured model path не разрешает удалять пользовательский каталог
или скачивать неиспользуемую вторую копию в другой путь. Создание нового стандартного
`.localvox` на Windows SHALL выставлять Hidden, не меняя атрибуты чужих/готовых папок.

Код: [resolver](../../../../../crates/localvox-light-core/src/model_paths.rs), [manifest](../../../../../scripts/models.json), [Windows fetch](../../../../../scripts/fetch-models.ps1), [Unix fetch](../../../../../scripts/fetch-models.sh).

#### Scenario: Fresh install
- **WHEN** пути не настроены, рядом с установкой/cwd нет моделей
- **THEN** downloader, CLI и doctor показывают один пользовательский каталог, а запись/проверка не создаёт его до необходимой загрузки.

#### Scenario: Existing external or portable models
- **WHEN** модель явно настроена вне установки либо существует в прежнем portable/CWD каталоге
- **THEN** найденный путь сохраняется, повторное скачивание и перенос не выполняются.

#### Scenario: Read-only resolution
- **WHEN** запрошены JSON paths или Check при отсутствующих моделях
- **THEN** вывод содержит выбранные пути/причины, файлы и атрибуты не меняются.

#### Scenario: Explicit invalid path
- **WHEN** заданный путь не содержит нужной модели
- **THEN** runtime/doctor сообщают именно об этом пути; обычная загрузка не подменяет его скрытым fallback и предлагает явную настройку/repair.

#### Scenario: Portable download override
- **WHEN** пользователь явно указал fetch root
- **THEN** компоненты загружаются в `<root>/models`, как до изменения, без создания пользовательского store.

#### Scenario: Unicode home and repeated run
- **WHEN** home содержит пробелы/Unicode, а выбранные модели уже готовы
- **THEN** все компоненты разрешают тот же путь, повтор не скачивает их и не создаёт другую копию.
