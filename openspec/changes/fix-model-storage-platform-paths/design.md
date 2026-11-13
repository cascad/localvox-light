# Design

## Context

См. proposal.md и hosted logs fee3103. Дефекты на границе файловой системы и shell;
новый runtime/провайдер не нужен. Есть tiny localhost PS/Bash fixtures.

## Goals / Non-Goals

Goals: восстановить уже требуемые Unicode/Hidden/default/repeat на штатных путях.
Non-goals: новые storage правила, миграция, inference, изменение UI или скрытие тестов.

## Decisions

- PS нормализует user root тем же GetFullPath, что component path. Так сравнение
  покрывает short-name и dot aliases; атрибуты existing root не меняются.
- Unix ZIP guard разрешает canonical ancestor aliases через pwd -P. Возвращённый
  physical destination используется для операции; отдельный symlink самой модели
  всё ещё запрещён, physical expected/target сверяются перед заменой. Запрет всех
  ancestor links не подходит: /var на Mac является штатной ссылкой, также допустим
  пользовательский home через ссылку. Hardcode /var не нужен.
- Curl получает только URL, а .part открывает shell redirection. Это устраняет
  перекодирование native Windows output filename без новой зависимости/скачиваний.
- Регрессии создают собственные aliases и проверяют неизменность внешнего содержимого;
  symlink privilege на Windows не предполагается, отдельный Unix сценарий остаётся
  честно skipped там, где нет native symlink. PS проверяется в 5.1 и установленном 7.x.

## Risks / Trade-offs

Path aliases → проверять canonical destination и не ослаблять запрет model symlink.
Hosted Mac недоступен локально → отдельная фиксация local/hosted, новый push владельца.
Не запускать user daemon/модели и не скачивать настоящие веса.

## Bash Hidden follow-up

Hosted 99bd709 исправил PS Hidden и curl; остался только Bash Hidden на runner.
Zip guard уже возвращает physical destination. new_model_parent должен приводить
Target и USER_MODEL_ROOT к той же physical форме перед prefix check, иначе короткий
путь/ancestor alias сравнивается с нормализованным destination и Hidden не ставится.
Использовать имеющийся physical_model_path, без hardcode runner path или смены атрибутов
existing папок. Добавить alias HOME regression, включая NTFS junction без admin.
