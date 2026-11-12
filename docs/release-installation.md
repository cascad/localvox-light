# Установка полного комплекта

Пакет содержит демон со встроенным браузерным UI, `localvox-process`, `localvox-tui`,
нативные библиотеки, `.env.defaults`, скрипты моделей и лицензии. На Windows/macOS
также собирается окно `localvox-desktop`. Рабочая `.env` при обновлении сохраняется.
Модели скачиваются отдельно установщиком; `RequiredOnly` пропускает optional модели.

## Из Release

Команды предназначены для нового полного release. Пока он не опубликован,
использовать [архив Actions](#свежая-сборка-из-actions); старый v0.1.5 не подходит.

Windows PowerShell, из каталога, где нужна папка `localvox-light`:

```powershell
$u='https://github.com/cascad/localvox-light/releases/latest/download/install-release.ps1'
$p=Join-Path $env:TEMP 'localvox-install.ps1'; Invoke-WebRequest $u -OutFile $p
powershell -NoProfile -ExecutionPolicy Bypass -File $p -Wizard
cd localvox-light
.\localvox-light.exe --daemon
```

macOS (Apple Silicon или Intel; нужен `jq`, например `brew install jq`):

```bash
curl -fsSL https://github.com/cascad/localvox-light/releases/latest/download/install-release.sh -o /tmp/localvox-install.sh
bash /tmp/localvox-install.sh --wizard
cd localvox-light
./localvox-light --daemon
```

UI: http://127.0.0.1:3017/. Во втором терминале из установленной папки запустить
`localvox-tui.exe` (Windows) или `./localvox-tui` (Mac). Для опциональной сводки
запустить Ollama и выбрать доступную модель в настройках. Для звука в TUI нужен
mpv ≥ 0.38 — см. `terminal-workspace.md` в комплекте.

## Свежая сборка из Actions

Пока новый полный релиз не опубликован, скачать артефакт нужной платформы из
[Build binaries](https://github.com/cascad/localvox-light/actions/workflows/build-binaries.yml).
Распаковать внешнюю ZIP-обёртку GitHub: внутри будет `localvox-<target>.zip` для
Windows либо `.tar.gz` для Mac. Передать именно внутренний архив установщику:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-release.ps1 -Archive 'D:\Downloads\localvox-x86_64-pc-windows-msvc.zip' -InstallDir 'D:\LocalVox' -Wizard
```

```bash
bash scripts/install-release.sh --archive="$HOME/Downloads/localvox-aarch64-apple-darwin.tar.gz" --install-dir="$HOME/LocalVox" --wizard
```

Для Intel Mac имя содержит `x86_64-apple-darwin`. Установщик взять из клона или
из этой же ветки: [Windows](../scripts/install-release.ps1) / [Mac](../scripts/install-release.sh).
После публикации он также доступен командой выше. Helper scripts и manifest моделей берутся из самого
пакета, той же версии. `-SkipModels` / `--skip-models` устанавливает только бинарники
и явно сообщает, что перед записью ещё нужны модели. Старые релизы с одним EXE
полным комплектом не считаются. Установка новой версии требует закрыть старые
клиенты и демон после завершения записи.

## Проверить или настроить повторно

Из папки установки:

```powershell
.\localvox-light.exe --setup-check
.\localvox-light.exe --setup
```

На Mac те же флаги у `./localvox-light`. `--setup-check` ничего не устанавливает,
не запускает серверы и не пишет файлы. `--doctor --doctor-json` выдаёт JSON schema 1
со стабильными ID компонентов; коды 0/1/2 означают готово/предупреждения/не хватает
обязательного. Проверка файлов не заменяет пробную запись и проверку качества модели.

В мастере Enter сохраняет обнаруженный выбор, `q` или конец ввода отменяет настройку.
Для недостающих ASR-моделей выбрать «готовая папка» или «скачать недостающее»;
корень загрузки можно указать на другом диске. Диаризация и GLiNER выбираются отдельно.
Папки используются напрямую, без копирования.

Для сводки выбрать Ollama: мастер прочитает список установленных моделей без их
запуска. Можно выбрать номер, `n` — скачать по имени, `g` — импортировать готовый
GGUF с новым именем. GGUF должен поддерживаться Ollama; импорт занимает место
в её хранилище дополнительно к исходному файлу. Существующее имя не заменяется.
Иной OpenAI-совместимый сервер задаётся адресом и именем модели; его API/авторизация
настраиваются отдельно. «Без автоматической сводки» сохраняет транскрибацию.

Если локальной Ollama нет, мастер предложит официальный установщик. На Windows
откроется его окно, на Mac — DMG: перенести Ollama в Applications и запустить.
Затем вернуться в терминал и нажать Enter. Существующая Ollama, её сервис и место
хранения моделей не меняются; выбранная модель скачивается в хранилище сервера.
Если CLI уже установлена, но сервер не отвечает, открыть приложение Ollama и повторить мастер.

Перед действиями будет план и подтверждение `yes`. Сначала остановить демон:
мастер не меняет используемые им модели и настройки. Отмена ничего не меняет.
После сбоя повторить мастер: готовые файлы и модели будут использованы повторно.
Обычный повтор без нового выбора не скачивает модели и не переписывает `.env`.

Проверка через установщик (работает даже до первой установки):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-release.ps1 -InstallDir 'D:\LocalVox' -Check
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-release.ps1 -InstallDir 'D:\LocalVox' -Reconcile
```

```bash
bash scripts/install-release.sh --install-dir="$HOME/LocalVox" --check
bash scripts/install-release.sh --install-dir="$HOME/LocalVox" --reconcile
```

Reconcile сверяет manifest и SHA-256 установленного комплекта, пропускает загрузку
и распаковку исправной текущей версии, затем открывает мастер. Это доустановка,
для обновления на другую версию запустить обычный установщик с `-Tag`/`--tag` или
`-Archive`/`--archive` без Reconcile. `-Reconcile -SkipModels` / `--reconcile --skip-models`
проверяет только комплект. Старый режим без мастера сохранён: `-RequiredOnly` /
`--required-only` скачивает обязательные модели, `-SkipModels` / `--skip-models` —
только программу. Этот автоматический режим использует каталог `models` в установке;
для готовых внешних моделей выбрать мастер.

## Проверка Mac владельцем

До фактического прогона все пункты **not-run**. CI-сборка не заменяет эту проверку.

- Пройти мастер с уже установленной Ollama и повторить его: без повторного скачивания.
- На чистой установке проверить официальный установщик Ollama, pull модели и импорт GGUF; отменить план до действий.
- Установить подходящий архив в папку с пробелом в имени; выполнить `./localvox-light --doctor`.
- Запустить `./localvox-light --daemon`, открыть UI и TUI, проверить доступ к одному архиву.
- Разрешить микрофон и нужные системные разрешения; проверить доступность loopback и выбранного выхода.
- Записать речь и системный звук, дождаться первой готовой части, проверить стыки и прослушивание.
- Stop: дождаться полного текста; закрыть UI/TUI и убедиться, что демон остаётся доступен.
- В терминале демона нажать `Ctrl+C`, снова запустить его: завершённая запись доступна, лишняя обработка не начинается.

Автозапуск после входа в систему и аппаратный capture на Mac отдельно не объявлены
проверенными. Для бага указать архитектуру Mac, версию ОС, действие и этап из журнала.
