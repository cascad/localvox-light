# Проверка · 2026-11-23

Владелец выбрал h/Esc и l/Enter **только в меню и списках**, сохранив h/l в
деталях и логах. Используются прежние back/enter и shortcut profiles; глобального
перехвата букв нет. Палитра также закрывается по h и выбирает по l, включая
прежнее отдельное подтверждение опасных действий. На корне h не завершает TUI.
Справка, палитра и вступление docs/user-guide.md обновлены.

## Test-run

Windows x64, locked dependencies, сборка и частные HTTP/terminal fixtures на D:.
Пользовательский демон, записи и модели не задействованы.

- TUI library: **242 passed / 3 ignored**, INSTA_UPDATE=no, без изменения снимков.
  Проверены все разделы, пустой архив, сохранение выбранной строки и звука по h,
  контекст Queue/Search, Notes/Ask/Settings, палитра и её подтверждение, отсутствие
  перехвата Ctrl/Alt/Shift, literal редактор/фильтр, английские и RU aliases.
  Прежние journal/console tests также passed: h/l продолжают сворачивать/раскрывать.
- I/O: **3 passed**, включая delayed DELETE и независимое управление записью.
- Настоящий ConPTY: **10 passed**. Startup-сценарий дополнен l→список→h→меню→l;
  сценарий удаления открывает подтверждение из палитры по l, не обходя s.
  Вспомогательная проверка выделения теперь ищет название только в панели меню:
  прежний поиск по всей строке принимал одноимённый заголовок документа и давал
  ложный timeout после возврата h. После исправления helper весь PTY suite прошёл.

Логи: target/review/note-deletion-20261123/navigation-tests.log (library и I/O
passed, первоначальный timeout PTY helper) и navigation-pty.log (10/10 passed).
Три ignored — дочерние test fixtures, не аппаратные успешные проверки.

Rustfmt изменённых файлов, git diff --check, strict main/change и check-openspec
прошли. YAML разобран; обе команды Mac gate ссылаются на существующие PTY tests.

Оба реальных PTY сценария включены в CI для каждой Mac архитектуры. Native Mac
CI и пользовательская проверка новых aliases **not-run** на момент этого отчёта;
локальные Windows результаты не выданы за Mac приёмку. Main specs не sync/archive.
