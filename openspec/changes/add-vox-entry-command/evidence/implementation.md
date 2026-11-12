# Проверка vox · 2026-11-12

SETUP-007 / RUN-005 реализованы в delta; main specs не sync/archive.

- Native `vox` использует общий terminal entry: portable cwd/.env, затем прежний
  HTTP client/bootstrap. Cargo normal dependency tree не содержит core/cpal/vosk/ort.
  `localvox-tui` сохраняет source/dev поведение.
- TUI release suite: 207 lib passed, 3 fixture-only ignored; 2 IO, 6 настоящих
  Windows ConPTY и 4 launcher/CLI passed. Media test требует mpv и явно ignored.
  Новый portable ConPTY повторно passed после проверки именно API_BIND из соседнего
  .env: запуск из другого каталога, чужая caller .env, выключенный LIGHT_TUI,
  reuse готового API без sibling daemon. Snapshot baselines не менялись.
- 7 registration tests passed: приватный Windows registry key (тип/raw PATH,
  Unicode/пробелы, expanded env entry, повтор), temp profiles, concurrent edit,
  denied replace с сохранением исходного файла, quoting/injection и double-source
  в настоящем Git Bash. Настоящий HKCU Environment и shell profiles не изменялись.
- Release terminal/vox build passed. Новый полный Windows zip содержит vox;
  29 delivery tests passed без skips, включая actual install/check/reconcile,
  повтор без перезаписи конфигурации и opt-out для изолированных тестов.
  Default registration wiring/conflict/error проверены fixture executable;
  сами registry operations — на приватном ключе.
- Installers регистрируют команду по умолчанию, check только читает; есть opt-out.
  Package smoke проверяет vox --version. CI запускает native profile tests на всех
  пяти targets, с Zsh/Fish на Unix. Bash syntax, PowerShell parse, actionlint,
  strict main/change specs, check-openspec и diff проверены отдельно перед commit.

Локальные логи: `target/tmp/vox-tui-all.log`, `vox-path-final.log`,
`vox-bind-config.log`, `vox-release-build.log`, `vox-delivery-native.log`.

Не проверено здесь: реальное изменение user PATH, native Mac/Linux, Unix mode/
symlink/Zsh/Fish tests и hosted jobs нового commit. Offline Linux cargo check
не выполнен: в локальном кеше отсутствует gethostname 1.1.0; это не pass.
Аппаратная запись и пользовательский daemon не запускались ради проверки.
Полный выход оформлен отдельно в add-graceful-application-exit; этот этап
сохраняет обычный выход только из TUI.
