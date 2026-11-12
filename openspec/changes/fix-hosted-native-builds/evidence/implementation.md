# Hosted native builds — 2026-11-12

## Проверенный push и история

Владелец выполнил fast-forward main до `371975d5c0adf60b8aa53494a68971f3f03d96ad`
и push. `git ls-remote` подтвердил этот SHA в main/new_course. Rebase и force push
не выполнялись. Этот follow-up commit создаётся локально на main, отдельно от
уже отправленной работы. Исторический отчёт предыдущего change не переписан.

## Hosted evidence для 371975d

[Build binaries 43245450999](https://github.com/cascad/localvox-light/actions/runs/43245450999):

- Windows job 131276748555 — **success**, включая complete bundle, installed
  doctor/check/setup/reconcile и upload. Linux ARM job 131276748807 — **success**.
- Linux x64 job 131276749154 — **failure** на link: undefined `__isoc23_strtol`,
  `__isoc23_strtoll`, `__isoc23_strtoull`, ссылки из ort-sys allocator/session/cast.
  Runner Ubuntu 22.04 несовместим с загруженной статической ORT distribution.
- Intel Mac job 131276749494 — **failure**: ort rc12 не предоставляет prebuilt
  для x86_64-apple-darwin. Это не ошибка модели или отключённого ASR.
- ARM Mac job 131276748970 — **cancelled**; logs API возвращает 404 BlobNotFound.
  Причина отмены неизвестна, результат не считается pass.

[Interface 43245450996](https://github.com/cascad/localvox-light/actions/runs/43245450996):
200 TUI lib tests, Unicode ConPTY и 5 workspace ConPTY **passed**. Следующий
run-script test упал на ложном `run.ps1 did not restore cwd`: argument содержит
`C:\Users\RUNNER~1`, а Get-Location возвращает полную запись того же пути.

После main push начались [Interface 43256472682](https://github.com/cascad/localvox-light/actions/runs/43256472682)
и [Build 43256472832](https://github.com/cascad/localvox-light/actions/runs/43256472832)
того же SHA. Во время проверки повторились те же 3 дефекта; ARM Linux прошёл,
Windows desktop / ARM Mac ещё выполнялись. Это не run новых исправлений.

## Исправления

- [Fixture](../../../../crates/localvox-light-terminal/tests/fixtures/run-script.ps1)
  сохраняет actual entered Get-Location и строго сравнивает его после launcher.
  [Тест](../../../../crates/localvox-light-terminal/tests/run_script.rs) с `.`
  воспроизвёл ошибку локально до fix; после fix проходят 9 launch cases и negative
  control script без восстановления cwd. Продуктовый run.ps1 не менялся.
- [Workflow](../../../../.github/workflows/build-binaries.yml): Linux x64 runner
  Ubuntu 24.04, как ARM; baseline указан в installation/development docs.
- [Intel Mac builder](../../../../scripts/build-onnxruntime-macos.py): тот же CPU
  ORT 1.24.2 из official commit `058787ceead760166e3c50a0a4cba8a833a6f53f`,
  native x64 / deployment 13.3 / jobs 2. Законченный SDK публикуется после проверки
  revision/architecture/library/licenses и SHA manifest. Cache reuse не вызывает
  команды/перезапись. Неполный/изменённый cache не затирается и не исполняется.
  Завершённый SDK сохраняется в Actions cache до Rust build; ключ включает helper hash.
- [Packager](../../../../scripts/package-release.py): repeatable `--extra-native`,
  dylibs и upstream notices входят в SHA manifest; существующий relocation и
  ad-hoc signing применяются ко всем библиотекам. Linker/SONAME aliases из SDK
  материализуются обычными файлами, как требует текущий link-free installer.
- README теперь ссылается на main и предлагает артефакт успешного platform job.

Primary sources проверены через GitHub connector:
[official tag](https://github.com/microsoft/onnxruntime/tree/058787ceead760166e3c50a0a4cba8a833a6f53f),
[ORT 1.24.2 assets](https://github.com/microsoft/onnxruntime/releases/tag/v1.24.2),
[upstream ort discussion](https://github.com/pykeio/ort/issues/556),
[official source-build instructions](https://onnxruntime.ai/docs/build/inferencing.html).
Локальные ort-sys rc12 dist.txt/dynamic_link.rs подтвердили отсутствие Intel
distribution и поддерживаемые ORT_LIB_PATH/ORT_PREFER_DYNAMIC_LINK. Старые
неофициальные Intel binaries и понижение runtime не применялись.

## Локальные test-run

TEMP/npm/cargo outputs на D:, jobs/test threads 2, пользовательские .env, архив,
модели и демон не затрагивались; никаких больших source builds на Windows/C:.

| Проверка | Результат |
|---|---|
| cwd alias до fix | **failed**, воспроизведён normalized-directory, остальные старые cases проходили |
| Final `cargo test --release --locked --offline -p localvox-light-terminal --test run_script` | **3 passed**, 0 ignored; 9 cases и negative control, clean bundle --version |
| `python -m unittest discover -s scripts/tests -p 'test_*delivery.py' -v`, native archive + Git Bash/jq | **27 passed, 0 skipped**, 49.184 s |
| SDK fixtures | В предыдущей строке 5 tests: pinned tag, architecture, build failure, corrupt/stale/unlisted/missing-license/null cache, reuse bytes+mtime; source compilation и Mach-O tools заменены fixture boundaries, это не native Mac build |
| Extra SDK packaging | Missing directory, copy/license/hash manifest, relative loader path, signing; actual Mach-O tools здесь fixtures |
| Fresh Windows CLI package | pass, --desktop; packaged daemon/worker/TUI startups; актуальный zip 50,966,441 bytes |
| App lib/setup integration / core doctor | **16 passed, 11 ignored** / **10 passed** / **6 passed**; ignored — существующие explicit child/native fixtures, не новый skip |
| Helper CLI --help / workflow static | pass; actionlint both workflows passed; Rust test scoped rustfmt passed |

Strict main specs — 15 passed / 0 failed; strict validation этого и предыдущего
showcase change, `python scripts/check-openspec.py` и `git diff --check` — passed.
Оба change включены в Specification checks interface workflow.

Ignored logs: `target/tmp/hosted-*`. Windows archive:
`target/setup-delivery-check/localvox-x86_64-pc-windows-msvc.zip`.

## Границы

Новый hosted run после этих правок, настоящий Intel source compile/link/start,
Linux x64 link на Ubuntu 24.04, ARM Mac completion, микрофон/loopback и звук —
**not-run** для данного change. Не выдавать эти fixtures за native acceptance.
Новый push должен отдельно запустить workflow; агент push не выполняет.
Матрица терминальных host UI и качество ASR/LLM этим срезом не проверяются.
