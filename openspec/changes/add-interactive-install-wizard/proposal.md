# Proposal

## Why

Владелец показал первый запуск: missing .env выдан как повреждённый файл, затем
технический doctor и числовые вопросы о Vosk/GigaAM. Нужен понятный установщик
одной строкой для Windows/Linux/macOS, без ручного скачивания GitHub artifacts.
Владелец хочет сначала посмотреть безопасный локальный имитатор.

## What Changes

- Bootstrap одной командой определяет платформу и сам получает полный комплект.
- Терминальный выбор «Рекомендуемая установка» / «Выборочная»; default готовит
  нужные модели/инструменты, устанавливает Ollama и выбранную модель при необходимости.
- Custom: настоящие checkbox, arrows/Space/Enter/Esc, человеческие названия,
  обязательное отдельно, рекомендации, найденное и размеры понятны до действий.
- По отзыву владельца: возврат «Назад» без перезапуска и потери подтверждённых ответов;
  Esc открывает меню навигации/отмены, итоговый план содержит явное действие «Назад».
- План перед подтверждением, reuse готового, отдельная диагностика вместо raw doctor
  на первом экране; отсутствие .env нормально, parse/access ошибки остаются видимыми.
- Сначала отдельный безопасный frontend preview без сети/записей; владелец смотрит
  его до подключения реальных действий и до GitHub build.
- Локальный стенд и сценарии для всех трёх ОС; native Linux/Mac владелец прогонит
  отдельно. Для bootstrap нужен публичный полный release, Actions auth пользователю
  не требуется. Публикация/tag/push отдельно после готовности и разрешения владельца.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

- installation-models: SETUP-009 interactive one-command install, safe preview и
  понятный first-run. SETUP-001/003 и delta SETUP-005/006/007/008 сохраняются.

## Impact

Отдельный лёгкий setup UI и preview, bootstrap/installers, setup planner/frontend,
dependency manifest, release delivery/tests, README/user-guide/evidence.
Preview не импортирует daemon/audio/ASR и не зависит от нативной Vosk библиотеки.
