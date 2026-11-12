# Proposal

## Why

Владелец запросил README с GIF в начале, короткой идеей/установкой и наглядными
возможностями, затем дополнительно исправить Actions и проверку установки/doctor.
Последний pushed `8a3a7dc` имеет реальные failed runs: Build binaries 43195753328
падает на missing emnapi lock dependencies; Interface 43195753300 продолжает после
ошибки npm и падает на Unicode native paste. README пока перегружен developer setup.

## What Changes

- README: GIF → идея → честный текущий способ установки → реальные возможности
  с подходящими PNG/GIF. Технические инструкции вынести в отдельный документ.
- Переиспользовать настоящие кадры с fictional data; отдельный promo index для
  дальнейших Telegram материалов, сам пост сейчас не создавать.
- Исправить npm lock/чистую установку, native error propagation CI, исследовать и
  исправить потерю supplementary Unicode в native input, сохранив strict test.
- CI запускает установленный комплект, check/JSON doctor, reconcile и повтор без
  записи; missing optional components имеют явные ожидаемые статусы, не blanket ignore.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

Нет. `skip_specs: true`: docs/build/CI и исправление ожидаемого Unicode input;
новых пользовательских действий/форматов нет. SETUP-001/002/003, SETUP-005/006
delta, ARCH keyboard/input и действующие scenarios сохраняются.

## Impact

README/docs/promo, workflows/package-lock, delivery fixtures, native input tests
и необходимый bug fix. Не публиковать post/release, не push без отдельного поручения.
Hosted green run после push и настоящий Mac остаются отдельной проверкой.
