# Design

## Context

Windows/macOS пакеты уже задуманы в build-binaries, но native staging не попадает
в архив. `build.rs` вычисляет неверную папку при явном native `--target`, поэтому
pack не должен полагаться на случайное копирование DLL в build hook.
Последний опубликованный v0.1.5 — старые отдельные бинарники. Новый релиз/push
не выполняется без поручения. Product capture/ASR/UI/TUI поведение сохраняется.

## Goals / Non-Goals

- Реальные читаемые PNG/GIF и воспроизводимая съёмка на synthetic fixture.
- Один полный пакет на платформу и единый проверяемый installer path.
- Запуск CI на рабочей ветке, проверка состава/loader до публикации.
- Не добавлять runtime demo mode; не использовать личный архив/микрофон.
- Mac hardware и новый hosted CI не объявлять прошедшими из Windows.

## Decisions

- Media: loopback fixture server с вымышленными встречами; настоящий React bundle
  в Chromium, настоящий TUI в portable-pty. ANSI отображается обычным terminal
  emulator для PNG/GIF. Keys подаются реальному процессу; подписи находятся снаружи
  продукта. Сценарии: navigation/filter, summary/transcript, journal/focus.
- Исходные кадры, видео и временные файлы лежат в target на D:, в Git — компактные
  конечные PNG/GIF и сценарии съёмки. Общий объём медиа ограничен и проверяется.
- Отдельный platform package tool собирает EXE/DLL либо executables/dylib, preset,
  licenses и pinned install/model scripts. macOS library references делаются
  переносимыми, проверка loader выполняется в CI на соответствующем runner.
- Installer выбирает точное имя архива, распаковывает во временную staging-папку,
  проверяет обязательный состав до копирования. Модели ставит существующий manifest
  loader из пакета, `.env` не перезаписывается. Локальный archive option даёт способ
  установки свежего Actions artifact до публикации release и служит для CI smoke.
- Windows и оба Mac runner остаются обязательными в matrix; Linux не удаляется.
  CI строит UI до Rust и проверяет готовый архив/установщик. PR и рабочие ветки
  получают проверки; tag/manual publish использует только полный набор пакетов.

## Risks / Trade-offs

Unsigned macOS binaries и реальные privacy permissions проверяются владельцем;
package smoke не запускает запись. Загрузка optional models/LLM не нужна для
loader smoke. Скриншоты демонстрационные, без обещаний модельного качества.

## Validation

Package/installer tests: полный/неполный состав, точный target, ошибки и сохранение
`.env`; реальный Windows bundle smoke. Syntax/lint workflow и scripts. Media
визуально проверяются после capture, GIF проверяются на кадры/размер/длительность.
Strict OpenSpec/diff и evidence; ручная Mac-приёмка остаётся unchecked.
