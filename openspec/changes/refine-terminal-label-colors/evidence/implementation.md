# Подписи и данные TUI · 2026-11-11

## Реализация

Theme.label и общие label_style/heading_style отделяют технические подписи от
содержимого. Панели/разделы/вкладки, подписи форм и настройки, служебные заголовки
документа/журнала используют приглушённый синий. Название записи и основной текст
остаются ink; значения настройки/редактора сохраняют value, пояснения — dim.
У неактивного поля label/value теперь отдельные Span вместо одинакового dim.
Ошибки/статусы, аргументы, активные команды и фокус сохраняют свои роли.

No-color использует Reset fg/bg и Dim/Bold/прежние маркеры; 16-color — только ANSI
palette. Тип подписи задаётся структурой renderer, без разбора пользовательского
текста по двоеточиям/регулярным выражениям. App/HTTP/pipeline не менялись.

## test-run

- Три новых теста проверяют реальный Buffer для dark/light/16/mono: captions,
  значения/пояснения, активное и неактивное поле, primary title/text и чистое copy.
- Расчёт RGB contrast подписи: dark 6.86:1 на bg / 5.18:1 на selection;
  light 5.52:1 / 4.78:1. Label/ink/value проверяются на обоих фонах, минимум 4.5:1.
- Просмотрены 40 обновлённых snapshots, дополнительно сравнено полное content:
  текст, строки, координаты/геометрия во всех них побайтно прежние; меняются styles.
  В dark/light/mono проверены примеры заголовков, settings, формы и selection.
- Финальный `cargo test --locked -p localvox-light-terminal --jobs 2 --
  --test-threads=2` с INSTA_UPDATE=no: 174 lib passed / 3 fixture helpers ignored;
  2 IO / 4 ConPTY / 2 run-script passed, hardware mpv ignored.
- Main specs strict: 15 passed; change strict, check-openspec и diff прошли.

## Поставка и граница

Release TUI успешно собран в обычный target/release/localvox-tui.exe (28.93 s),
обновлён также прежний target/review/vim-navigation/localvox-tui.exe. Оба --version
проходят, SHA-256 совпадает:
`967ef04c016e94a6c8beae1bfe68fff3e0ae93b70927d456e8e39f7058b8ae19`.
Процессы владельца не завершались, демон/архив/настройки не изменялись.
Реальный внешний вид на терминале владельца — not-run; автоматический Buffer/PTY
не заменяет его субъективный просмотр. Delta ARCH-015 не синхронизирована и change
не архивирован. Решение сохранять открытие Enter/l остаётся прежним.
