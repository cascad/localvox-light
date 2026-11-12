# Design

## Context

Doctor уже использует loader path resolution, fetch-models использует models.json.
Требуется соединить их, а не дублировать разрешение путей в PowerShell/Bash.
Официальная установка Ollama управляет собственным приложением и хранилищем.

## Goals / Non-Goals

- Один Rust planner/prompt/probe/executor в приложении; shell scripts только bootstrap
  и platform actions. Не требовать Python/Node/Rust у установленного пользователя.
- No-op повтор и read-only check, внешние модели без копирования.
- Не менять runtime обработки, не запускать ASR/LLM inference, не переносить
  хранилище существующей Ollama, не переписывать пользовательские комментарии.
- Не изобретать daemon/service management для Ollama: её официальный installer
  может потребовать действие в окне ОС, мастер затем повторно проверяет состояние.

## Decisions

- `--setup` — terminal wizard, `--setup-check` — read-only проверка;
  `--doctor --doctor-json` — versioned machine report с component IDs.
- Новый setup module разделён на typed choices/plan, read-only probes,
  prompt frontend и execution. Doctor model checks используются напрямую.
- План различает Keep / existing path / download. Keep не закрепляет defaults
  новыми overrides. Только изменённые ключи сохраняются в прежнюю .env.
- Новые модели скачиваются существующими manifest loaders только для выбранного
  component/root. Полные существующие модели не передаются загрузчику; частичный
  ZIP чинится явным repair действием. Ошибки остаются ошибками.
- Apply получает существующий workspace ownership lock перед изменениями;
  работающий daemon не останавливается автоматически. Check не получает write lease.
- Ollama tags читаются через API, список не запускает модель. Pull/create только
  для отсутствующего явно выбранного tag; выбранный GGUF не заменяет существующий
  tag. HTTP endpoints нормализуются без credentials/query, токены не печатаются.
- Установка Ollama — отдельный platform helper, официальный installer/download
  в выбранный local cache. Нужное действие в официальном окне завершается владельцем;
  существующий server не перезапускается. GGUF import может занимать дополнительное
  место в хранилище Ollama, это видно до подтверждения.
- Install scripts: Check ранний и без scratch; Wizard после bootstrap, Reconcile
  проверяет manifest существующего пакета и не распаковывает исправную установку.
  Старые noninteractive/SkipModels/Archive сценарии сохраняются.

## Risks / Trade-offs

Bootstrap без бинарников может проверить только их отсутствие; общий doctor доступен
после установки полного bundle с native dependencies. Mac first-run installer и
реальные большие downloads остаются отдельной приёмкой; тесты используют fixtures.
