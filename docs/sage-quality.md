# Проверка SAGE на CPU

Инженерный прототип вне приложения. Модель скачана по поручению владельца в
`F:\project_models\sage-fredt5-95m`; окружение — `F:\project_models\.venvs\sage95m`,
кеши — `F:\project_models\.cache`. Веса и окружение в git не добавляются.

Модель `ai-forever/sage-fredt5-distilled-95m`, revision
`ed51b4a46603931380951a3d8456685c9215864f`, веса SHA-256
`e005b48ae460cbf5f82f0f3da8b13d5df7c2c98eb1e02ed8a7d525d3de7a9bfe`.
Скачаны также config/generation_config, vocab/merges и tokenizer configs.
В актуальном HF CLI для нескольких файлов используйте явный список filenames,
а не несколько значений после одного `--include`.

Для нового окружения с установленным Python 3.12:

```powershell
$env:UV_CACHE_DIR = 'F:\project_models\.cache\uv'
$env:HF_HOME = 'F:\project_models\.cache\huggingface'
uv venv --python 3.12 F:\project_models\.venvs\sage95m
uv pip install --python F:\project_models\.venvs\sage95m\Scripts\python.exe torch==2.9.1 --index-url https://download.pytorch.org/whl/cpu
uv pip install --python F:\project_models\.venvs\sage95m\Scripts\python.exe transformers==4.57.1
```

Запуск на уже скачанных файлах; output должен быть новым:

```powershell
$env:HF_HUB_OFFLINE = '1'
$env:PYTHONDONTWRITEBYTECODE = '1'
F:\project_models\.venvs\sage95m\Scripts\python.exe scripts/profile-sage-quality.py --model-dir F:\project_models\sage-fredt5-95m --output target/sage-cpu-new.json --threads 2 --repeats 3
```

Один процесс загружает модель один раз; каждая реплика передаётся отдельно, её номер
задаёт код. Сетевых загрузок и CUDA в этом тесте нет. Два corpus по умолчанию:
72 основных случая и 24 boundary/dialogue. `--corpus` можно задать несколько раз.
Генерация greedy, без усечения входа; слишком длинный вход вызывает отказ, отсутствие
EOS отмечается как незавершённый результат. Отчёт содержит source/actual; не публикуйте
личные тексты. `success` означает завершение измерения, а не принятие качества.

Это **raw model test без production guards/receipts**, поэтому он не равен измерению
приложения через Ollama. Отдельно считаются 8 format/asr-repair ожиданий, соответствующих
назначению SAGE. Удаление пауз/повторов и summary не объявлены её возможностями.
Изменение кавычек может нарушить буквальный oracle без потери слов; такие случаи
разбираются отдельно. Найденная замена повеления прошедшим временем — смысловая ошибка.

На Mac CPU-путь ещё не запускался; MPS и экспорт encoder/decoder в ONNX — not-run.
При успешном качестве можно проверять Rust/ONNX-вариант; сейчас модель в приложение
не подключена. [Результаты отбора](../openspec/changes/screen-portable-cleanup-models/evidence/results.md).
