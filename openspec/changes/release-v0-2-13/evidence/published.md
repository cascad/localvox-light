# v0.2.13 опубликован и проверен · 2026-11-25

- [Публичный release](https://github.com/cascad/localvox-light/releases/tag/v0.2.13),
  `published_at = 2026-11-25T11:23:28Z`; GitHub `latest` возвращает v0.2.13,
  `draft=false`, `prerelease=false`.
- Неизменённый аннотированный тег: `6e8a993735498eb29731aac2986a7f0c15b3396d`.
  Удалённый peeled ref повторно сверен через `git ls-remote`.
- [Релизный run 45596000962](https://github.com/cascad/localvox-light/actions/runs/45596000962)
  завершён с `conclusion=success`, `head_sha` совпадает с тегом.

## Фактические gates

Все завершились успешно: release delivery preflight, public model sources,
Windows interface checks, пять native build/package/install jobs и публикация.
Никакие jobs этого релизного run не перезапускались.

Native jobs проверяют упакованный комплект, doctor/setup/reconcile, готовые пути
моделей, регистрацию vox и клавиатуру мастера на своих ОС. Mac jobs также
выполнили отдельные PTY-сценарии удаления/меню. Интерфейсный gate выполнил
browser/visual, ConPTY, синтетическую запись/очередь, настоящий mpv, общий
HTTP-сценарий UI/TUI и строгие spec checks.

## Публичные вложения

Ровно 14 непустых uploaded assets: пять архивов, пять `.sha256` и четыре скрипта
`install.ps1`, `install.sh`, `install-release.ps1`, `install-release.sh`.

| Архив | Bytes | SHA-256 из GitHub metadata и скачанного sidecar |
|---|---:|---|
| localvox-x86_64-pc-windows-msvc.zip | 123027662 | f2670ffa12cd0a041c9abba1308d2f1940c9c4f547c6218bae93bfe075d3d965 |
| localvox-x86_64-unknown-linux-gnu.tar.gz | 45806176 | 6c3934761ca372e1b5cbf1009dc2fcfe20021836cb3a9d410fd54f6de4e3c169 |
| localvox-aarch64-unknown-linux-gnu.tar.gz | 41734619 | eb6d412124211be8c032db5c224eedd40cd299102207881d2dd429bb68bd37e3 |
| localvox-x86_64-apple-darwin.tar.gz | 52294282 | ef81f6360359f067a48e009b1692ca08f5df4d4e47bdcf7f2013bcdf5e48b800 |
| localvox-aarch64-apple-darwin.tar.gz | 42326556 | 1467753f879c83700549831b30423489f107e2f5f3122ab00b3b89fc6b85c8e0 |

Независимо скачаны все пять checksum и четыре install-скрипта. Проверены их
размеры/digest, checksum → archive digest и полное побайтовое совпадение скриптов
с `git show <tag-sha>:scripts/<name>`. Архивы повторно локально не скачивались:
полные hashes вычислил и проверил publish job до публикации, затем GitHub
подтвердил digest/размер каждой загрузки.

Локальное свидетельство: `target/review/release-published-verification.json`,
`release13-published-verification.log` (игнорируются Git).

## Границы и текущий main

`5a6f5d6` исправляет только область auth-локатора теста; этот коммит не входит
в тег, Runtime/React-код относительно тега не менялся. Усиленный сценарий с двумя
auth-панелями passed локально. Дополнительный
[полный interface run текущего main 45602889523](https://github.com/cascad/localvox-light/actions/runs/45602889523)
на момент проверки публикации ещё выполняется; он не выдан за completed.

Полный portable `test-behavior.py` на пользовательском Mac/Linux, реальные
разрешения/микрофон/loopback и качество/скорость моделей этим выпуском не
проверялись. Отдельные native CI-сценарии выше не заменяют аппаратную приёмку.
Для запуска владельцем — [behavior testing](../../../../docs/behavior-testing.md).
