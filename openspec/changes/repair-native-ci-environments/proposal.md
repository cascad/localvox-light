# Proposal

## Why

На 2346911 Windows build и interface CI падают в ort-sys copy-dylibs после
автоматического перехода stable с Rust 1.99.1 на 1.100. Mac builds проходят
компиляцию, но fixture скрывает Homebrew Fish собственным PATH до запуска shell.

## What Changes

- Зафиксировать поддерживаемый Rust 1.99.1 общим toolchain-файлом для dev/CI:
  именно он прошёл все пять native jobs на 8900c58. Проверять совпадение CI с pin.
- Разрешать executable Zsh/Fish через исходный PATH до изоляции child env.
  Прогон fixture с shell вне /usr/bin:/bin остаётся обязательным, без skips.
- Проверить реальные Windows build/link/package и изолированные shell fixtures;
  сохранить hosted failures и границы последующего hosted rerun в evidence.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

Нет. Исправляется delivery tooling по RUN-002, SETUP-002/003 и SETUP-007 delta.
`skip_specs: true`: runtime, ONNX Runtime 1.24.2/орт API, модели и пользовательские
настройки не меняются. Новый runtime SDK 1.31 требует отдельной проверки inference
и не входит в устранение двух build/fixture defects.

## Impact

rust-toolchain.toml, оба workflows, terminal registration tests/dev dependency,
delivery checks и development guide, evidence/coverage. Без правки registry sources,
создания фиктивных target dirs или ослабления тестов. Push и hosted run нового
commit не выполняются без отдельного поручения.
