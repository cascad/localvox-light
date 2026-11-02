# Tasks

## 1. Регрессии и admission

- [x] 1.1 Воспроизвести RV-018/RV-019 failing tests на коротких оговорках, цитатах, наклонении и соседних строках; реализовать общий ограниченный guard с полезными typo/пунктуационными/ASR/glossary сценариями и duplicate-ID fallback; пройти LLM unit tests.
- [x] 1.2 Проверить настоящим worker + fake local provider refine/cleanup: опасные предложения отклонены, хорошая правка принята, первичная версия/тайминги/receipts сохранены; сохранить результаты.

## 2. Native качество и стоимость

- [x] 2.1 Повторить ordinary cleanup на прежних corpus/границах с Gemma/Qwen, проверить сохранность и usefulness; отдельно replay сохранённых опасных предложений и SAGE-примеров через guard, не выдавая replay за новые native calls.
- [x] 2.2 Проверить стоимость guard на публичном длинном тексте без запуска ASR, обновить честные пределы RV-018/RV-019 и пользовательские строки; пройти targeted Rust/Python, strict main/change, check-openspec и diff, синхронизировать только проверенные delta и закоммитить этап.
