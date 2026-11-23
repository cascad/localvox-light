# Проверка · 2026-11-23

## Источник дефекта

Владелец установил v0.2.7 на macOS и сообщил отсутствие модели по пути
`~/Applications/LocalVox/models/gigaam-v3-e2e-ctc`. По его `ls` модель находится
в `~/.localvox/models/gigaam-v3-e2e-ctc`: ONNX 224893347 байт, yaml и vocabulary.
Это пользовательское свидетельство, не запуск агента на Mac.

Config координатора имел отдельный старый resolver exe/cwd/models. Worker CLI,
doctor и установщик уже используют model_paths::Context. Вызов worker получает
явный --model-dir, resident ASR — Request.model_dir; оба брали неверный Config path.
Исправлен composition boundary, не модель/веса и не приоритет явных настроек.

Регрессия сначала **упала** на home: ожидался private home/.localvox/models,
получен cwd/models. После подключения общего resolver тест passed для home,
portable, cwd, quoted relative override и missing explicit override без fallback.
Передача единственного Config.model_dir в оба transport проверена по коду
execution.rs и warm.rs; настоящий ASR inference и захват на Mac **not-run**.

## Счётчик ошибок

Повторы в консоли на скриншоте не доказывают новые запуски ASR: Data::Queue
повторно записывал один Failed snapshot при каждом poll. Добавлен optional
execution_id в API/client DTO. Одинаковое исполнение не создаёт новых отказов;
новая попытка отличается даже при тех же error/attempts. Старые DTO читаются.
PIPE-010 и очередь не менялись: бюджеты попыток/возвращений сохранены.

API тест подтвердил стабильный ID в последовательных ответах. TUI тест прогоняет
по 100 свежих poll с возрастающими request ID: один отказ, repeats=1; затем новая
попытка, ещё 100 poll и recovery. Проверены новый и legacy DTO, unread semantics.

## Клавиши и копирование

Console: j/k и стрелки выбирают событие; h/l и стрелки Left/Right идемпотентно
сворачивают/раскрывают повторы; y копирует выбранную запись с контекстом и
известными повторами, Ctrl+Y — всю историю. PgUp/PgDn прокручивают длинный текст.
Journal: существующие j/k/h/l дополнены копированием выбранной группы с сессией,
этапом, состоянием, командой/деталями и известной длительностью.

State tests проверяют выбранную копию, отсутствие соседних записей в буфере,
полную копию, повторные h/l, Home/End, русский shortcut profile и текстовый фильтр.
Настоящий ConPTY проверяет ll → раскрытие, hh → сворачивание, l → раскрытие.
Clipboard проверен как Task::Copy; системный буфер владельца не изменялся.
Девять snapshots проверены: изменены только строки подсказок и их границы цвета.

## Выполненные проверки

Windows x64, native target, без debug symbols/incremental cache; временные файлы
и сборки на D:, модели не скачивались. Логи: target/review/recording-fix-20261123.

- Main lib: **23 passed / 12 ignored**; install **12**, setup **11**, model_paths **1**.
- API lib: **94 passed / 8 ignored**; client lib **3**.
- Core lib, без ONNX feature: **458 passed / 9 ignored** (включая бюджеты очереди).
- TUI lib: **227 passed / 3 ignored**; настоящий ConPTY: **9 passed**.
- Последующая проверка английских/русских клавиш — passed; финальный TUI rerun:
  **227 passed / 3 ignored**, включая известную длительность в выбранной копии.
- Native daemon/installer/TUI/vox собраны; help/version smoke passed, версия
  **0.2.8**. UI build passed. Rustfmt изменённых файлов, diff, строгие main/change
  specs и check-openspec passed.

Игнорируемые hardware/model/helper сценарии не выданы за аппаратную приёмку.
Новая модель не испытывалась; чистая Mac установка/захват с исправленной сборкой
ожидают проверки владельца. Исправление войдёт в v0.2.8; push/tag разрешены
владельцем отдельно 2026-11-23. Статус GitHub сборки не следует из локальных тестов.
