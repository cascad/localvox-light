# Доставка v0.2.0

SETUP-001/002/003 и SETUP-006/009 (delta). [Локальная реализация и проверки](integration.md)
сохранены отдельно; этот отчёт фиксирует следующую границу проверки.

## Commit, tag и publication

Владелец самостоятельно выполнил `git push`, затем `git tag v0.2.0 4514678` и
`git push origin v0.2.0`. Агент не переписывал историю и не выполнял новый push.
Tag указывает на `45146789ab5314df19347379be02eae171c5267e`.

- [Main Build binaries](https://github.com/cascad/localvox-light/actions/runs/43587750355): success.
- [Main Interface contracts and visuals](https://github.com/cascad/localvox-light/actions/runs/43587750359): success.
- [Tag Build binaries + publication](https://github.com/cascad/localvox-light/actions/runs/43630571901):
  все пять build jobs и Publish GitHub Release — success.
- [Release v0.2.0](https://github.com/cascad/localvox-light/releases/tag/v0.2.0): опубликован
  `2026-11-14T19:45:48Z`, не draft/prerelease, стал latest. Доступны пять архивов,
  пять `.sha256`, `install.ps1`, `install.sh` и два compatibility installer scripts.

## Native CI

| Target | Job | Результат |
|---|---|---|
| Windows x64 | [132556029720](https://github.com/cascad/localvox-light/actions/runs/43630571901/job/132556029720) | success |
| Linux x64 | [132556029714](https://github.com/cascad/localvox-light/actions/runs/43630571901/job/132556029714) | success |
| Linux ARM64 | [132556029466](https://github.com/cascad/localvox-light/actions/runs/43630571901/job/132556029466) | success |
| macOS Intel | [132556029618](https://github.com/cascad/localvox-light/actions/runs/43630571901/job/132556029618) | success |
| macOS Apple Silicon | [132556029709](https://github.com/cascad/localvox-light/actions/runs/43630571901/job/132556029709) | success |

На каждом target пройдены Delivery contracts, Native vox registration, Installer
keyboard/configuration contracts, Pack and probe, installed doctor/reconciliation
и external-model reuse. Это реальные native runners и PTY/ConPTY/filesystem/HTTP
fixtures, но без speech inference, физических устройств и установки больших моделей.

## Публичные assets и запуск Windows one-liner

- Обе `releases/latest/download/install.*` доступны. SHA-256 скачанных bootstrap
  совпадают с GitHub asset digests; публичный `install.sh` прошёл `bash -n`.
- Пять публичных `.sha256` прочитаны по pinned `v0.2.0` URL. Каждая строка содержит
  имя своего архива и hash, совпадающий с digest опубликованного archive asset.
  Это сверка metadata/checksum для всех targets; все пять архивов локально заново
  не скачивались. Windows ZIP отдельно реально скачан и проверен самим bootstrap.
- На Windows в настоящем ConPTY выполнена ровно команда из README:

  ```powershell
  irm https://github.com/cascad/localvox-light/releases/latest/download/install.ps1 | iex
  ```

  Для прогона HOME/USERPROFILE/LOCALAPPDATA/APPDATA/TEMP перенаправлены в новый
  private root под `target/review/release-v0.2.0` на D:. Унаследованные LOCALVOX
  настройки убраны; LLM discovery направлена на закрытый localhost port, без
  обращения к пользовательскому серверу. Драйвер отвечает на DSR терминала.
- Bootstrap разрешил latest в v0.2.0, скачал полный ZIP и его checksum, загрузил
  native installer с DLL, открыл настоящий экран «Выбрать компоненты».
  На первом экране отправлен Esc; процесс завершился кодом 0 с «Отменено».
  Проверено отсутствие целевой `Programs/LocalVox`, `.localvox` и staging-папок.
  Confirm не отправлялся: модели/инструменты/PATH/.env не устанавливались и не менялись.
- Локальный протокол: `target/tmp/public-bootstrap-check.log`, raw ConPTY transcript
  и временный драйвер в `target/review/release-v0.2.0`. Эти артефакты ignored и не
  входят в commit; production код ради прогона не менялся.

## Оставшаяся ручная приёмка

Реальная рекомендуемая установка на чистом пользовательском окружении с OS manager,
настоящая установка Ollama/pull/GGUF, системные запросы прав, физические микрофон,
loopback и плеер на Mac/Linux/Windows — **not-run** этим прогоном.
Cancel smoke и зелёный CI не заменяют эту проверку. Ссылки на опубликованные assets
не означают, что скачаны/прогнаны все модели.

Архивирование/sync delta не выполнялись. Доставка завершена; аппаратные ограничения
остаются видимыми и после закрытия задачи 3.1.

Правка этого этапа затрагивает документацию: strict main specs (15/15), strict change,
`check-openspec.py` и scoped diff прошли. Повтор speech/model inference не требуется
и не выполнялся; production код и tag v0.2.0 не изменены.
