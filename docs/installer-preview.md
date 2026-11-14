# Демо установщика

Это безопасный просмотр нового мастера: ничего не скачивает, не запускает Ollama
и не меняет настройки, PATH, модели или записи. Найденные компоненты здесь фиктивные.
Реальная установка одной строкой для Windows/Linux/macOS ещё не подключена.

Из корня репозитория в Windows:

```powershell
.\target\review\installer-preview\localvox-setup-preview.exe
```

`↑/↓` — выбрать, `Пробел` — изменить галочку, `Enter` — дальше, `Esc` — отменить.
В конце тоже можно отменить. Размер терминала — от 80 колонок; удобнее 110.

Посмотри два пути:

1. «Установить всё необходимое» → итоговый план → подтверждение демо.
2. «Выбрать компоненты» → галочки → источник сводки, если она включена → папки → план.

Для сценария «всё уже установлено» добавь `--scenario existing`.
Пустое текстовое поле принимает подсказанное значение по Enter; чтобы заменить его,
просто начни вводить новое. В демо сервер и файл GGUF не проверяются.

Windows/Linux/macOS из исходников, с уже установленным Rust:

```sh
cargo run --locked -p localvox-setup-ui --bin localvox-setup-preview -- --scenario fresh
cargo run --locked -p localvox-setup-ui --bin localvox-setup-preview -- --scenario existing
```

Это команды разработческого демо. Конечному пользователю Rust не понадобится.
Нативный прогон на Linux/macOS пока не выполнен.

## Воспроизвести проверки и снимки

```powershell
$env:LOCALVOX_INSTALLER_CAPTURE_DIR = "$PWD/target/review/installer-preview/captures"
cargo test --locked -p localvox-setup-ui -- --test-threads=1
node scripts/installer-preview/capture.mjs
```

В Bash вместо первой строки: `export LOCALVOX_INSTALLER_CAPTURE_DIR="$PWD/target/review/installer-preview/captures"`.
Снимки и GIF появятся в `target/review/installer-preview/`. Рендер использует уже
установленные зависимости `ui` (Playwright, xterm, шрифт) и ffmpeg, без их загрузки.
Тесты используют временные папки; вывод снимается с настоящего PTY/ConPTY.
