# Design

## Context

См. proposal.md. Существующий manifest содержит paths `models/<component>/...`;
fetch Root — родитель models. Runtime GigaAM ищет env/exe/cwd, CLI Vosk имеет
строковый default, installer пишет абсолютные install/model overrides.
Переиспользуем эти контракты, меняя implicit default.

## Goals / Non-Goals

**Goals:** один resolver в core, общий manifest placement для Rust/скриптов,
готовые external/portable модели не дублируются; read-only resolution тестируется.

**Non-Goals:** миграция существующих данных, установка моделей в реальный профиль,
изменение размеров/весов/ASR, перенос Ollama, новое хранилище записей/конфигурации.

## Decisions

- В model manifest добавить user-directory `.localvox`, env и directory каждого
  компонента. Это общий источник имён папок; payload/hash остаются прежними.
- Resolver получает home/exe/cwd и overrides отдельным context, чтобы тесты
  не меняли глобальную среду. Production использует std home discovery, на Windows
  USERPROFILE. Приоритет: explicit, sibling ASR для side моделей, существующий
  exe/CWD каталог, пользовательский default. Разрешение ничего не создаёт.
- `--model-paths-json` — ранняя maintenance ветка без TUI, модели и сетевого probe.
  Downloader установленного bundle получает от неё paths/source, включая .env;
  standalone scripts без binary используют тот же manifest и env/legacy/home rule.
  Старый binary с несовместимым helper — ошибка, не тихий fallback.
  При наличии .env standalone требует explicit Root: без native resolver нельзя
  гарантировать одинаковое чтение dotenv, а shell source/eval недопустим.
- Explicit Root downloader сохраняет прежнюю семантику и не зависит от профиля.
  При default вызове каждый payload привязан к resolved component directory.
  Configured broken directory требует явного выбора/repair через мастер; его
  содержимое нельзя молча заменить. Zip destructive target checks остаются обязательны.
- Мастер предлагает home/.localvox как download root; выбранный путь сохраняется
  существующими per-model env overrides. Новая установка сохраняет archive path,
  model overrides не прикалывает к install; готовая .env остаётся побайтно прежней.
- Default .localvox создаётся lazily при записи выбранного payload; Windows Hidden
  выставляется только новому standard root. Check/ready paths не создают даже root.

## Risks / Trade-offs

- Три языка → placement data в manifest, native JSON для bundle, реальные PS/Bash
  fixture/parity tests; не парсить .env shell eval/source.
- Новый default мог бы зацепить личные веса в тестах → fake home/USERPROFILE под
  временной папкой во всех новых downloader/install tests, никакой настоящей загрузки.
- Неоднозначная legacy папка → сохранять прежний directory selection и явно
  показывать source; отсутствие файлов остаётся диагностикой, не скрытым переносом.
- Windows recursive operations → проверить абсолютный target под выбранным
  component destination; не удалять arbitrary configured roots.

## Migration Plan

Автоматической миграции нет. Готовые explicit/portable paths сохраняются. Для нового
размещения пользователь выбирает download в мастере; plan/confirm остаются прежними.
