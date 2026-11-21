# v0.2.5 — публикация и полная чистая установка Windows

2026-11-21. SETUP-001/003/009 (delta), INV-SETUP-001, PIPE-008/011.
Продолжение [локального полного прохода](full-windows-install.md).

## Публичный выпуск

Исходный коммит: `3475d54767963cc7df710c41cce54e5333645521`.
[v0.2.5](https://github.com/cascad/localvox-light/releases/tag/v0.2.5)
опубликован в `2026-11-21T09:21:59Z` и стал latest. Старые теги не переносились.

- [Release workflow 44885842650](https://github.com/cascad/localvox-light/actions/runs/44885842650):
  публичные источники, пять native targets, Interface и публикация — success.
- [Main Build 44885832301](https://github.com/cascad/localvox-light/actions/runs/44885832301)
  — success; [main Interface 44885832232](https://github.com/cascad/localvox-light/actions/runs/44885832232)
  — success после одного повторного запуска упавшего job.
  Исходный отказ сохранён: Chromium сообщил `net::ERR_NO_BUFFER_SPACE` при
  загрузке `/src/journal.css`; старт приложения не завершился, поэтому fixture
  не увидел heading за 7 с. Trace скачан и изучен до повтора. Тесты/таймауты и
  код не изменялись; релизный Interface прошёл с первой попытки.
- Ровно 14 assets: пять архивов, пять SHA-256-файлов, четыре install-скрипта.
  Публичные checksum сверены с GitHub asset digests, четыре настоящих
  `latest/download` скрипта — с digest и содержимым исходного коммита.

## Реальный чистый проход

В настоящем Windows ConPTY запущен опубликованный PowerShell bootstrap:

```powershell
& ([scriptblock]::Create((irm https://github.com/cascad/localvox-light/releases/latest/download/install.ps1))) -NoPathUpdate
```

HOME/USERPROFILE/APPDATA/LOCALAPPDATA/TEMP перенаправлены в ранее отсутствовавший
профиль под `target/review/release-v0.2.5/fresh-home` на D:. Рабочая папка тоже
частная, унаследованные LOCALVOX-переменные удалены. `-NoPathUpdate` защищает
настоящую команду vox владельца. Публичные URL/архивы/модели не подменялись.

Выбран custom с пятью возможностями, Ollama `qwen3.5:9b` и рекомендуемыми папками.
План перед подтверждением просмотрен: все четыре группы моделей требуют загрузки,
ни одна не берётся из F:\lv или репозитория. Готовые системные ffmpeg/yt-dlp и
Ollama CLI/API/модель использованы повторно; mpv поставляется в проверенном ZIP.

- Публичный архив скачан, SHA проверен, мастер прошёл до `LocalVox готов. Запуск:`;
  exit 0 за 219 с, включая ответы/просмотр плана и реальные загрузки.
- Заново скачаны и установлены Vosk, GigaAM, segmentation/embedding и GLiNER
  с tokenizer/config. Все восемь файлов с опубликованными в каталоге SHA-256
  независимо перечитаны целиком и совпали. У Vosk проверен `am/final.mdl`;
  upstream SHA-256 не публикует, такой гарантии проверка не приписывает.
- Установленный `--version`, manifest и `--bundle-check --release-tag v0.2.5`
  подтвердили версию/целостность. Pending checkpoint удалён.
- Установленный `localvox-install --check` — exit 0.
  `localvox-light --doctor --doctor-json` — exit 0, все 11 findings `ok`.
- Индикатор обновляется одной строкой с временем; детальные строки разделены.
  Повторяющихся `Выполняется · …` и посторонних `пропущено` в transcript нет.
- Тот же публичный bootstrap запущен повторно в том же частном профиле:
  актуальный v0.2.5 проверен, скачивание архива пропущено, все компоненты готовы;
  подтверждённый проход — exit 0 за 22 с. Нет модельных загрузок, размеры/mtime
  всех файлов моделей и SHA-256 активного .env не изменились. Doctor повторно 0.

Первый вариант независимого verifier ошибочно ожидал девять findings setup-check
в расширенном runtime doctor. Фактически оба doctor уже завершались с 0, runtime
содержал 11 `ok`; ожидание исправлено на точный набор его 11 ID и проверка повторена.
Это ошибка стенда, не отказ установленного продукта.

Скрипты проверки, ANSI/transcripts, review, manifest/checksums, doctor и snapshots
сохранены в игнорируемом `target/review/release-v0.2.5`. Приватный .env не коммитился.
Этим закрыт gate полной чистой публичной Windows-установки. Native CI Mac/Linux
прошёл; полная загрузка моделей/OS-пакетов на этих ОС, новая установка Ollama на
чистой ОС и аппаратная запись/права устройств этим проходом не подтверждаются.
