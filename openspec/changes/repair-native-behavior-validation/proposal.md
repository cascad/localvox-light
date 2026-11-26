# Proposal

## Why

Полный локальный ARM Mac отчёт c11c213 показал шесть failed групп и отсутствие
GUI dependencies. В одном отчёте смешались Python 3.10, отсутствующий fish,
установленный TUI 0.2.11 против исходников 0.2.13 и реальные платформенные
разрывы. Пользователь поручил разобрать архив и исправить причины с регрессами.

## What Changes

- Ранняя проверка Python 3.11+, зависимостей групп и версий установленного клиента;
  несовместимый PTY-клиент не запускается против новых сценариев и не подменяется.
- Shell registration имеет отдельный явный шаг с zsh/fish prerequisites;
  остальные state/render tests проверяются независимо от наличия этих shells.
- ARCH-001 явно отвергает Windows prefixes, оба разделителя и нормализуемые пути
  на любой ОС, сохраняя чтение правильных session IDs.
- Settings/cross-client fixtures сравнивают канонические пути; ранний выход
  settings child сохраняет диагностику вместо потери stderr.
- Installation fixture использует только собственный model root, сохраняя
  производственное повторное использование уже установленных моделей.
- Native API regressions добавлены к существующим macOS build jobs; инструкции
  US-067 и evidence отражают prerequisites и пределы проверок.
- После повторного Mac прогона: shared UI/TUI/API fixture компилируется на всех
  поддерживаемых ОС; проверяются его регистрация и выполнение в Unix CI.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

- `archive-playback`: уточнение ARCH-001 для одинакового отклонения путей
  между поддерживаемыми ОС; исходное намерение и проверка Windows prefixes сохранены.

## Impact

API archive catalog; copied-executable test fixtures; installation tests;
portable behavior runner, его тесты/отчёты, macOS CI и документация.
Рабочий архив, демон, пользовательские модели и установленный комплект не меняются.
