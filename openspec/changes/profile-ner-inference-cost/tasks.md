# Tasks

## 1. Измерить текущую реализацию

- [x] 1.1 Добавить opt-in NER components/counters без изменения обычных API/вычислений; model-free tests подтверждают порядок, точные результаты/ошибки, finite/exclusive times, partial/privacy и отсутствие clocks в disabled path; документировать API.
- [x] 1.2 Подключить --ner-components к grounding profiler с ordinary parity без core observer, обновить engineering docs/CI; unit tests проверяют numeric report, частичный отказ и границы включения, CLI help/сборка проходят.

## 2. Сверить реальные компоненты

- [x] 2.1 Выполнить явный native Entity parity test и два bounded fresh release профиля прежней публичной пары без LLM; сохранить численные component/counter/hash/parity evidence с неизменностью входов/моделей и пределами вывода.
- [x] 2.2 Обновить текущий статус, catalog/coverage LLM-004/PIPE-007 и вывод о следующем шаге; targeted tests/rustfmt, strict main/change, check-openspec и diff проходят; коммитить только проверенный этап.
