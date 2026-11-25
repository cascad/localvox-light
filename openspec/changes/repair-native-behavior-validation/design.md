# Design

## Context

Вход — ARM Mac report 20261125T114827-25043, исходники c11c213 с dirty tree,
установленные vox/TUI 0.2.11. Полные причины и границы — в evidence; чужие правки
dirty tree не считаются известными. См. proposal для перечня наблюдений.

## Goals / Non-Goals

**Goals:** отделить unsupported prerequisites от реальных failures до долгих
сборок; сохранить проверки каждой доступной группы; исправить ARCH-001 и
платформенную изоляцию fixtures.

**Non-Goals:** установка пакетов на чужом Mac, автоматическое обновление клиента,
подмена установленного бинарника исходниками, изменение повторного использования
моделей, реальные устройства/модели или выпуск нового тега.

## Decisions

- Python 3.11+ — общий prerequisite: при старом runtime формируется отчёт с
  blocked и not-run, сборки не стартуют. Не поддерживаем частично 3.10 через shims.
- Установленные клиенты сравниваются с workspace version. Неподходящая версия
  блокирует только использующие её PTY/cross-client шаги; source tests независимы.
- Внешний zsh/fish тест вынесен в отдельный шаг runner с requires-tools. Обычные
  unit tests runner исключают только этот конкретный тест, отдельный шаг явно
  passed/blocked; прямые Cargo/CI проверки не ослабляются.
- Portable archive validation выполняет явную проверку запрещённых символов до
  платформенного Path::components, чтобы нормализация не меняла границу доступа.
- Owned child fixtures сравнивают канонические cwd/exe/root. Settings child пишет
  stdout/stderr в собственный временный файл; отказ готовности включает этот лог.
- Model fixture задаёт явные component paths из manifest в discovery и конфигурации назначения.
  Production reuse остаётся прежним; готовый чужой HOME не влияет на ожидания теста.
- Существующие macOS jobs после setup tests запускают archive/settings API tests,
  используя уже собранные native dependencies.

## Risks / Trade-offs

- Версия бинарника не доказывает точный SHA → отчёт сохраняет source SHA/dirty
  tree и установленную версию раздельно; совпадение не заменяет реальные тесты.
- Windows не воспроизводит alias /tmp → canonical ownership регресс проверяется
  через fixture helper/symlink где доступно; native Mac результаты остаются not-run
  до повторного фактического запуска, без заявления о прошедшей аппаратной приёмке.
- Наличие shell может измениться между preflight и тестом → actual process errors
  всё равно дают failed; отсутствие не превращается в passed.
