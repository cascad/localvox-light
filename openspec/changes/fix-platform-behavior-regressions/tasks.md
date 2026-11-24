# Tasks

## 1. Жизненный цикл записи

- [x] 1.1 Исправить global request generation без отмены глобальных мутаций при смене документа; state/Io и настоящий PTY delete→новая запись с тем же title→poll без перезапуска, stale response regression, обновить US-011/023.
- [x] 1.2 Добавить UUID к новым ID, проверить одинаковые title/удаление/recreate и cleanup зависимостей через allocator/API tests; legacy archive read сохраняется, обновить US-003/023.
- [x] 1.3 Убрать архивный pre-roll и его подсказки, сохранить bounded orphan только после Start; synthetic PCM обоих источников до/во время/после Start/Stop, legacy настройки и длительность, обновить US-002/003.

## 2. Управление и звук

- [x] 2.1 Единые s/ы confirmations и Enter для однострочных форм, n/p/RU/Ctrl+n/p в палитре; state/render/PTY regressions без перехвата букв редактора, обновить US-011/018 и помощь.
- [x] 2.2 Исправить mpv loading/EOF/границу фрагмента; native null-audio IPC tests Load/Toggle/Seek/EOF/replay и явный отказ при настоящей потере процесса, обновить US-015 при изменении поведения.

## 3. Локальный поведенческий прогон

- [x] 3.1 Общий runner Windows/macOS/Linux: фиксированные группы native state/HTTP/PTY/recording/API/mpv и GUI, ограниченные ресурсы/логи, изолированные фикстуры; unit tests runner для success/failure/timeout/missing dependencies/report/handoff и документация одной команды.
- [x] 3.2 Выполнить доступные локальные группы и strict OpenSpec/check-openspec/diff, записать факт Windows прогона; Mac/Linux local not-run до фактического отчёта, команды для владельца и местного агента проверены на fixtures; коммитить только связанные проверенные этапы.
