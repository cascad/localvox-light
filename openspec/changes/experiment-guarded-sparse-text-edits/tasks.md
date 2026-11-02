# Tasks

## 1. Консервативный протокол

- [x] 1.1 Реализовать opt-in sparse adapter с exact anchors, JSON schema, ограничениями и rollback; проверить malformed/foreign/duplicate/overlapping edits, числа/отрицания/содержательные слова/инъекции и неизменённый ordinary forwarding model-free tests, описать ограничения.
- [x] 1.2 Подключить только к text-only cleanup и сохранить numeric counters/полный набор строк для прежних worker guards; настоящий worker + fake local LLM должен подтвердить безопасный fallback, source/metadata/receipts и отсутствие текста в отчёте.

## 2. Модели и полезность

- [x] 2.1 Создать короткий synthetic corpus с положительными и защищёнными случаями; выполнить baseline/candidate на установленных 9B, 4B и 3B, проверить protected invariants и gate ≥75% однозначных noise cases, сохранить скорость/ошибки/полезность отдельно от JSON validity. Доступная после установки пользователем Gemma — дополнительный кандидат, не обязательная зависимость.
- [x] 2.2 При пройденных коротких gates измерить публичный двухчасовой cleanup baseline/candidate на выбранной маленькой модели с сохранением hashes/counters; при провале gate зафиксировать отрицательный результат и причину отказа от длинного/production перехода.

## 3. Выводы и трассировка

- [x] 3.1 Обновить evidence LLM-003/PIPE-006 и engineering docs по фактическим результатам; отделить сокращённую capability от паритета, оставить качество вне набора непроверенным, пройти strict main/change, check-openspec, formatting и diff.
