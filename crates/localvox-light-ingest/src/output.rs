//! Резолвер путей результата для одного / нескольких источников.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// Параметры для [`resolve_output_paths`].
///
/// * Если `count == 1` — возвращается ровно один путь к файлу (`output` или
///   `output_base/single_filename`, либо `single_filename` в cwd).
/// * Если `count > 1` — каталог из `output_dir` / `output` (если это каталог) /
///   `output_base` / `default_multi_dir`, и в нём `<multi_prefix>_001.txt`, …
pub struct OutputSpec<'a> {
    pub count: usize,
    pub output: Option<&'a Path>,
    pub output_dir: Option<&'a Path>,
    pub output_base: Option<&'a Path>,
    pub default_multi_dir: &'a str,
    pub single_filename: &'a str,
    pub multi_prefix: &'a str,
}

pub fn resolve_output_paths(spec: OutputSpec<'_>) -> Result<Vec<PathBuf>> {
    let OutputSpec {
        count,
        output,
        output_dir,
        output_base,
        default_multi_dir,
        single_filename,
        multi_prefix,
    } = spec;

    if count == 1 {
        let p = if let Some(o) = output {
            o.to_path_buf()
        } else if let Some(dir) = output_base {
            dir.join(single_filename)
        } else {
            PathBuf::from(single_filename)
        };
        if p.is_dir() {
            anyhow::bail!(
                "для одного источника --output — путь к .txt файлу, не каталог"
            );
        }
        return Ok(vec![p]);
    }

    let dir = match (output_dir, output, output_base) {
        (Some(d), _, _) => d.to_path_buf(),
        (None, Some(o), _) if o.is_dir() => o.to_path_buf(),
        (None, Some(o), _) if !o.exists() && o.extension().is_none() => o.to_path_buf(),
        (None, Some(_), _) => {
            anyhow::bail!(
                "несколько источников: укажите --output-dir КАТАЛОГ (или существующий каталог в --output)"
            );
        }
        (None, None, Some(d)) => d.to_path_buf(),
        (None, None, None) => PathBuf::from(default_multi_dir),
    };

    std::fs::create_dir_all(&dir)?;
    Ok((0..count)
        .map(|i| dir.join(format!("{multi_prefix}_{:03}.txt", i + 1)))
        .collect())
}
