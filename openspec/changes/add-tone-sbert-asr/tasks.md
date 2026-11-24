# Tasks

## 1. Native модели

- [x] 1.1 Добавить T-one engine отдельно от mel GigaAM: resampling/state/padding/CTC и строгий IO; unit boundary/reset/silence и настоящий pinned model parity с официальным greedy; записать evidence и ограничения decoder.
- [x] 1.2 Добавить отдельный native SbertPuncCase processor с официальными весами/word-safe tokenization; короткий/длинный/числа/кириллица/label failures и logits/labels parity; исходный текст не перезаписывается, evidence реальных моделей отдельно.

## 2. Pipeline и область выбора

- [x] 2.1 Зафиксировать backend/пунктуацию для новой сессии, legacy GigaAM, явная recook текущим выбором после ready-check; package/recipe/identity/worker early/final одинаковы. Regression old/new/manual/active/invalid model без массовой переварки, обновить user-guide.
- [x] 2.2 Интегрировать производную пунктуацию с сохранением исходной версии и раннего текста; отдельный cache/операции/время/конечный отказ. Проверить partial→Stop/restart/disable/failure и сохранность audio/time/words через worker/API fixtures.

## 3. Установка и настройки

- [x] 3.1 Добавить pinned tone/sbert components в общий manifest/resolver/doctor и optional выбор setup/vox setup: download/готовые пути/active backend, default GigaAM, повтор без двойной работы. Проверить локальный wizard preview/native PTY и HTTP download/hash/interruption/reconcile fixtures на доступной ОС; обновить user-guide.
- [x] 3.2 Общие настройки UI/TUI для ASR и пунктуации с точным сроком применения/готовностью; state/render/browser regressions, выбор двух скачанных моделей не запускает два ASR; обновить user-guide.

## 4. Интеграционная проверка

- [x] 4.1 Включить новые поведенческие сценарии в portable runner, прогнать Windows native/GUI/installer/model parity последовательно, strict OpenSpec/check/diff; Mac/Linux hardware и длинный quality benchmark честно not-run без фактических отчётов. Коммитить проверенные этапы без неразрешённого push.
