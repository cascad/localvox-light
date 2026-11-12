# Design

## Context

См. proposal. Версия ort rc12 ожидает API 24 и штатно использует ORT 1.24.2.
Его dist.txt не содержит Intel Mac. Microsoft прекратил готовые Intel archives
после 1.23; официальный source build остаётся описан. Linux static distribution
ссылается на glibc __isoc23_*; Ubuntu 22.04 их не предоставляет.

## Goals / Non-Goals

Сохранить version/API и CPU-провайдер, доставить все нативные зависимости рядом
с бинарниками, не тратить повторные запуски на готовый runtime. Не делать shim
glibc, не понижать ORT ради Intel и не скачивать third-party prebuilt.

## Decisions

- Linux x64: Ubuntu 24.04, как ARM job; документировать baseline вместо обещания
  Ubuntu 22.04. Контракт платформ не закреплял прежнюю версию glibc.
- Только Intel Mac: pinned официальный ORT source/tag+commit, Release shared CPU
  library, macOS deployment 13.3 и jobs 2. Завершённый cache содержит dylibs,
  upstream licenses и SHA manifest; повреждённый/неизвестный output не затирать.
- Cache restore/save отдельно до Rust: законченный runtime сохраняется, даже
  если последующий app build выявит другой дефект. Key содержит helper hash.
- Официальные ORT_LIB_PATH/ORT_PREFER_DYNAMIC_LINK задаются только в этом job.
  Packager получает дополнительный native каталог, копирует runtime/licenses,
  использует прежние otool/install_name_tool/codesign и проверяет старты после
  переноса. Глобальное окружение пользователя не меняется.
- Fixture сравнивает cwd с тем, что вернул Get-Location сразу после Push-Location,
  а не с ненормализованным аргументом. Добавить путь с `.` (воспроизводит локально)
  и отрицательный контроль launcher без Pop-Location, чтобы не ослабить контракт.

## Risks / Trade-offs

- Intel first build дольше → cache завершённого SDK, jobs 2, bounded subprocess
  timeout. Большой build исполняется только на CI Mac, не на пользовательском C:.
- Скрытая dylib dependency → прежний fail-closed relocation и packaged startup.
- Source-build проверен локально только orchestration/fixtures → native Mac и
  hosted результат явно not-run; Windows/ARM уже прошли предыдущий hosted run.
- Mac ARM cancelled run имеет недоступные logs (404); причину не выдумывать.
