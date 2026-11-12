use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::i18n::Language;

pub fn resolve_library_root(path: &Path) -> PathBuf {
    let nested_ldraw = path.join("ldraw");
    if nested_ldraw.is_dir() { nested_ldraw } else { path.to_path_buf() }
}

// ── Structs ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Settings {
    pub color_correction: bool,
}

impl Default for Settings {
    fn default() -> Self { Settings { color_correction: true } }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub library_paths: Vec<PathBuf>,
    pub settings: Settings,
    pub output_dir: Option<PathBuf>,
    pub max_history: usize,
    pub language: Language,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            library_paths: Vec::new(),
            settings: Settings::default(),
            output_dir: None,
            max_history: 10,
            language: Language::Zh,
        }
    }
}

// ── sRGB helpers ──────────────────────────────────────────────────────────────

/// sRGB → Linear 转换 (gamma 2.2)
pub fn srgb_to_linear(c: f32) -> f32 { c.max(0.0).powf(2.2) }

/// 对 RGB 三通道做 sRGB → Linear
pub fn srgb_to_linear_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [srgb_to_linear(rgb[0]), srgb_to_linear(rgb[1]), srgb_to_linear(rgb[2])]
}

// ── Config file ───────────────────────────────────────────────────────────────
//
// Minimal storage format (no readability needed, edited only via TUI):
//   cc=true
//   D:\LDraw
//   C:\Program Files\Studio 2.0\ldraw
//
// First line starting with "cc=" is the color-correction flag.
// Every other non-empty line is a library path.

pub fn save_config(exe_dir: &Path, cfg: &Config) {
    let mut out = String::new();
    out.push_str(if cfg.settings.color_correction { "cc=true\n" } else { "cc=false\n" });
    out.push_str(&format!("max_history={}\n", cfg.max_history));
    out.push_str(&format!("language={}\n", cfg.language.as_str()));
    if let Some(ref d) = cfg.output_dir {
        out.push_str(&format!("output_dir={}\n", d.to_string_lossy()));
    } else {
        out.push_str("output_dir=\n");
    }
    for p in &cfg.library_paths {
        out.push_str(&format!("{}\n", p.to_string_lossy()));
    }
    if let Err(e) = std::fs::write(exe_dir.join("config.toml"), out) {
        eprintln!("[WARN] 无法写入 config.toml: {e}");
    }
}

pub fn load_config(exe_dir: &Path) -> Config {
    let config_path = exe_dir.join("config.toml");
    if !config_path.exists() {
        std::fs::write(&config_path, "cc=true\n").ok();
        return Config::default();
    }
    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[WARN] 无法读取 config.toml: {e}");
            return Config::default();
        }
    };
    let mut cfg = Config::default();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some(v) = line.strip_prefix("cc=") {
            cfg.settings.color_correction = v == "true";
        } else if let Some(v) = line.strip_prefix("max_history=") {
            if let Ok(n) = v.parse::<usize>() { if n >= 1 { cfg.max_history = n; } }
        } else if let Some(v) = line.strip_prefix("language=") {
            cfg.language = Language::from_str(v);
        } else if let Some(v) = line.strip_prefix("output_dir=") {
            if !v.is_empty() { cfg.output_dir = Some(PathBuf::from(v)); }
        } else {
            // Skip any remaining key=value line (key = plain identifier [a-zA-Z0-9_]+).
            let is_kv = line.find('=')
                .map(|i| line[..i].chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(false);
            if !is_kv {
                cfg.library_paths.push(PathBuf::from(line));
            }
        }
    }
    cfg
}

fn index_dir(base: &Path, dir: &Path, index: &mut HashMap<String, PathBuf>) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&cur) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            // Key by relative path (forward slashes, lowercase)
            if let Ok(rel) = path.strip_prefix(base) {
                let rel_key = rel.to_string_lossy().replace('\\', "/").to_lowercase();
                index.entry(rel_key).or_insert_with(|| path.clone());
            }
            // Key by filename only
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                index.entry(name.to_lowercase()).or_insert_with(|| path.clone());
            }
        }
    }
}

/// Build a combined LDraw index from:
///  1. custom_parts/ folder next to exe (highest priority – user patches)
///  2. cli_ldraw_path (--ldraw-path argument)
///  3. config library_paths
///  4. Fallback: ldraw/ folder next to exe
///
/// Returns (index, total_files_indexed, warnings)
pub fn build_combined_index(
    exe_dir: &Path,
    config: &Config,
    cli_ldraw_path: Option<&Path>,
) -> (HashMap<String, PathBuf>, usize, Vec<String>) {
    let mut index: HashMap<String, PathBuf> = HashMap::new();
    let mut warnings: Vec<String> = Vec::new();

    // 1. custom_parts/ next to exe (highest priority – do first so or_insert keeps these)
    let custom_folder = exe_dir.join("custom_parts");
    if custom_folder.is_dir() {
        index_dir(&custom_folder, &custom_folder, &mut index);
    }

    // Collect all library roots to scan
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(p) = cli_ldraw_path {
        roots.push(resolve_library_root(p));
    }
    for path in &config.library_paths {
        roots.push(resolve_library_root(path));
    }
    let fallback = exe_dir.join("ldraw");
    if fallback.is_dir() {
        roots.push(fallback);
    }

    // 2-4. Scan all library roots
    for root in &roots {
        if !root.exists() {
            warnings.push(format!("路径不存在，已跳过: {}", root.display()));
            continue;
        }
        if !root.is_dir() {
            warnings.push(format!("不是文件夹，已跳过: {}", root.display()));
            continue;
        }
        for subdir in ["parts", "p", "s"] {
            let d = root.join(subdir);
            if d.is_dir() {
                index_dir(&d, &d, &mut index);
            }
        }
        index_dir(root, root, &mut index);
    }

    let count = index.len();
    (index, count, warnings)
}
