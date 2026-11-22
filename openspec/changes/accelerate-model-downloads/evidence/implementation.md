# Проверка, 2026-11-22 · Windows x64

SETUP-010 и INV-SETUP-001: реализация проверена локально; main specs ещё не sync.
Новая публичная установка и native Linux/macOS в этом проходе **not-run**.

## Полная загрузка

Vosk ru 0.42: **1 937 602 113 байт**, SHA-256
`00eb9e5ed8230492da1acf702cd3548e1282256a5fdde8f188253e77217e9429`.
Полностью скачаны и сверены официальный ZIP alphacephei.com и зеркало
[super-boy/vosk-models, revision 18e6f93](https://huggingface.co/super-boy/vosk-models/resolve/18e6f9301a85eeb82377d922bbc3983a062401c4/vosk-model-ru-0.42.zip?download=true).
Зеркало общественное; совпадение установлено по полным архивам, а не HEAD/префиксу.
В каталоге закреплены revision, размер и SHA; официальный URL — резервный.
Модель не уменьшалась и не заменялась другой версией.

Настоящий `fetch-models.ps1 -Only vosk -Root <изолированный каталог F:>` с новым
native helper: скачивание и потоковая проверка **44 с**, примерно **41,7 MiB/s**;
вместе с распаковкой и проверкой `am/final.mdl` — **63,64 с**, exit 0.
Повтор — **0,21 с**, без скачивания. ZIP и распаковка находились рядом с каталогом
модели на F:, временных каталогов после завершения нет. Это измерение одной сети,
не гарантия времени для любого подключения. Продукт использует один HTTP-поток;
восемь диапазонов использовались только при отдельной сверке официального reference.

Локальные исходные результаты: `target/review/model-delivery-2026-11-22/`:
`official-reference.json`, `full-native-install.{log,json}`, `repeat-native-install.log`.
HTTP source gate для нового primary тоже passed. Автоматическая проверка дважды
заблокировала удаление reference ZIP и изолированной proof-модели на F: с причиной
`blocked by policy`, включая вариант с заранее проверенными фиксированными путями.
Они оставлены в `F:\project_models\localvox-download-cache`; пользовательские модели
и архив не затронуты. Обход блокировки не выполнялся.

## Проверки реализации

- Native release: main lib **22 passed / 12 ignored**, install **12**, setup **11**,
  ASR lib **1**. Локальный HTTP: redirect, резерв после HTTP error, известный и
  неизвестный размер, короткое тело, неверный SHA, сохранность готового файла.
  Ollama: общий byte protocol, verification/success и недопустимые счётчики.
- Python: **26 tests, 3 skipped, success** (PS5 и Git Bash с jq включены).
  Новый native helper дополнительно проверен через оба настоящих скрипта:
  fallback 401 → ZIP с тем же SHA, повтор без HTTP, байты без утечки frames в
  обычный вывод; protocol mode передаёт байты и этап распаковки. Эти ветки passed.
  Старый/отсутствующий helper сохраняет standalone fallback.
- Setup UI: lib **1**, ConPTY **12 passed**; fragmented UTF-8 frames, общий
  индикатор, известный/неизвестный размер, unpack, обычный вывод без ANSI/frames.
  Новый сценарий проверен на **80 и 110 колонках**, реальные ConPTY captures
  отрендерены и просмотрены. `--download-demo` не читает конфиг и не меняет файлы.
- Собраны daemon, worker, TUI, vox, installer и preview с `--release --locked`,
  native Windows target и ONNX feature unification через пакет ASR. UI build passed.
- Strict change/main specs, `check-openspec.py`, rustfmt изменённых файлов и diff passed.

## Текущая установка

Обнаружен старый `F:\lv\vox.exe` 0.2.6 без команды setup. Обновлены пять CLI
исполняемых файлов, defaults, каталог моделей и оба download-скрипта. Предыдущие
файлы сохранены в локальном review. `.env` и пользовательские модели/архив сохранены.
`vox --version` → **0.2.7**, `vox setup --help` → 0, bundle-check → 0.
`vox setup --check` совпал с прямым installer check: **1** (предупреждения о
неустановленных необязательных компонентах), без изменения конфига и архива.

Локальный комплект помечен `v0.2.7-local`: существующее semver-сравнение возвращает
3 для предлагаемого v0.2.6 и 2 для будущего стабильного v0.2.7. Поэтому старый
публичный релиз не откатывает эту проверочную установку, новый стабильный заменит её.
Это локальная сборка, не утверждение о публикации v0.2.7 на GitHub.
