//! The frontend (`ui/dist`) is embedded into the binary at compile time. It is built
//! by npm, and npm is not a Rust dependency — so a machine without Node must still be
//! able to `cargo build`.
//!
//! If the build is missing, we put a page in its place that says how to make one. The
//! alternative — a compile error from inside the embedding macro — points at the macro,
//! not at the cause, and tells nobody what to do about it.

use std::fs;
use std::path::Path;

const PLACEHOLDER: &str = r#"<!doctype html>
<meta charset="utf-8">
<title>localvox — интерфейс не собран</title>
<style>body{font:15px/1.6 system-ui;margin:3rem auto;max-width:40rem;padding:0 1rem}
code{background:#eee;padding:.15rem .35rem;border-radius:4px}</style>
<h1>Интерфейс не собран</h1>
<p>Бинарь собран без фронтенда. Соберите его и пересоберите демон:</p>
<pre><code>npm --prefix ui ci
npm --prefix ui run build
cargo build --release --workspace</code></pre>
<p>Старая страница архива пока работает: <a href="/legacy">/legacy</a>.</p>
"#;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist");
    println!("cargo:rerun-if-changed={}", root.display());

    if root.join("index.html").exists() {
        return;
    }
    if let Err(e) = fs::create_dir_all(&root).and_then(|()| fs::write(root.join("index.html"), PLACEHOLDER)) {
        println!("cargo:warning=ui/dist placeholder: {e}");
    }
    println!("cargo:warning=ui/dist is empty — run `npm --prefix ui run build`");
}
