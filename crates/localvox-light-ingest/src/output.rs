//! The resolver of result paths for one / several sources.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// The parameters for [`resolve_output_paths`].
///
/// * If `count == 1` — exactly one path to a file is returned (`output` or
///   `output_base/single_filename`, or `single_filename` in the cwd).
/// * If `count > 1` — a directory out of `output_dir` / `output` (if it is a
///   directory) / `output_base` / `default_multi_dir`, and in it
///   `<multi_prefix>_001.txt`, …
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
            anyhow::bail!("for a single source --output is a path to a .txt file, not a directory");
        }
        return Ok(vec![p]);
    }

    let dir = match (output_dir, output, output_base) {
        (Some(d), _, _) => d.to_path_buf(),
        (None, Some(o), _) if o.is_dir() => o.to_path_buf(),
        (None, Some(o), _) if !o.exists() && o.extension().is_none() => o.to_path_buf(),
        (None, Some(_), _) => {
            anyhow::bail!(
                "several sources: pass --output-dir DIRECTORY (or an existing directory in --output)"
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
