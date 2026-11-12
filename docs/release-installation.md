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
powershell -NoProfile -ExecutionPolicy Bypass -File $p -RequiredOnly
cd localvox-light
.\localvox-light.exe --daemon
```

macOS (Apple Silicon или Intel; нужен `jq`, например `brew install jq`):

```bash
curl -fsSL https://github.com/cascad/localvox-light/releases/latest/download/install-release.sh -o /tmp/localvox-install.sh
bash /tmp/localvox-install.sh --required-only
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
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-release.ps1 -Archive 'D:\Downloads\localvox-x86_64-pc-windows-msvc.zip' -InstallDir 'D:\LocalVox' -RequiredOnly
```

```bash
bash scripts/install-release.sh --archive="$HOME/Downloads/localvox-aarch64-apple-darwin.tar.gz" --install-dir="$HOME/LocalVox" --required-only
```

Для Intel Mac имя содержит `x86_64-apple-darwin`. Установщик взять из клона или
из этой же ветки: [Windows](../scripts/install-release.ps1) / [Mac](../scripts/install-release.sh).
После публикации он также доступен командой выше. Helper scripts и manifest моделей берутся из самого
пакета, той же версии. `-SkipModels` / `--skip-models` устанавливает только бинарники
и явно сообщает, что перед записью ещё нужны модели. Старые релизы с одним EXE
полным комплектом не считаются. Установка новой версии требует закрыть старые
клиенты и демон после завершения записи.

## Проверка Mac владельцем

До фактического прогона все пункты **not-run**. CI-сборка не заменяет эту проверку.

- Установить подходящий архив в папку с пробелом в имени; выполнить `./localvox-light --doctor`.
- Запустить `./localvox-light --daemon`, открыть UI и TUI, проверить доступ к одному архиву.
- Разрешить микрофон и нужные системные разрешения; проверить доступность loopback и выбранного выхода.
- Записать речь и системный звук, дождаться первой готовой части, проверить стыки и прослушивание.
- Stop: дождаться полного текста; закрыть UI/TUI и убедиться, что демон остаётся доступен.
- В терминале демона нажать `Ctrl+C`, снова запустить его: завершённая запись доступна, лишняя обработка не начинается.

Автозапуск после входа в систему и аппаратный capture на Mac отдельно не объявлены
проверенными. Для бага указать архитектуру Mac, версию ОС, действие и этап из журнала.
