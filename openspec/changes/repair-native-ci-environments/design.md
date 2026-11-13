# Design

## Context

См. proposal. Логи 43291458096/43291458202 показывают Rust 1.100 и новый
OUT_DIR `build/ort-sys/<fingerprint>/out`. ort-sys rc.12 использует ancestors(3),
получает каталог build вместо release и не доставляет DLL к executable.
Последний пяти-платформенный pass 43262427328 использовал Rust 1.99.1.

## Decisions

- rust-toolchain.toml — единственный pin 1.99.1 + rustfmt, minimal profile.
  Оба workflows читают channel штатным Python tomllib в step output и передают
  его в dtolnay/rust-toolchain@master; build matrix сохраняет свой target. Regression test
  проверяет источник версии и отклоняет moving stable/different overrides.
- Современный upstream ort rc.15 решает layout, но меняет SDK с 1.24.2 на 1.31.
  Это отдельная migration с inference проверкой. Здесь используем поддерживаемую
  воспроизводимую сборку rc.12, без локального fork/registry patch и directory hacks.
- Unix fixtures сначала получают абсолютный shell через which, затем ограничивают
  PATH ребёнка. Which нужен только Unix dev tests; shell HOME/profile isolation
  и обязательные проверки Zsh/Fish сохраняются.
- Local native verification на 1.99.1: отдельные RUSTUP_HOME/target на D, без
  изменения пользовательского rustup default. Build terminal/daemon/worker, startup
  и package smoke, relevant tests. Hosted failures не переписываются как pass.

## Risks / Trade-offs

- Новая версия компилятора требует осознанного обновления pin и полного native CI.
  Ожидание этого шага явно описано в developer guide; `stable` не обновляет его молча.
- Mac execution доступен в CI; локальные shell fixtures/checks не выдавать за
  native Mac pass. Новый hosted run требует push владельца.
