# Tasks

## 1. Сохранение работы при повторе

- [x] 1.1 Воспроизвести full recook из кнопки очереди, реализовать Failed → Pending той же фазы с rollback/новым временем/бюджетом, отдельный HTTP/client retry под lease/mutex; проверить фазы, отказ записи/чтения, сохранность артефактов и истории, import без audio, busy/recording/non-Failed и двойной POST, записать evidence.
- [x] 1.2 Подключить UI кнопку и TUI R/палитру с защитой повторного нажатия, обновлением статуса и прежними recook; проверить pending/error/success/формы/выбранную сессию и native/browser контракты, описать управление в docs.

## 2. Интеграция и свидетельства

- [x] 2.1 Выполнить релевантные core/API/client/TUI tests, полный UI unit/build/lint/e2e с прежними визуальными эталонами и общий temp API/UI/TUI; обновить coverage/catalog/CI, strict main/change/check-openspec/format/diff и предложить имя ручного коммита.
