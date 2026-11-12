mod color_resolver;
mod config;
mod i18n;
mod terminal_settings;
mod fbx_exporter;
mod geometry_builder;
mod io_reader;
mod ldraw_parser;
mod types;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use clap::Parser;
use colored::Colorize;

use color_resolver::{drain_unknown_color_warnings, load_ldconfig_from_roots};
use config::resolve_library_root;
use geometry_builder::{ExportMode, GeometryBuilder};
use i18n::Lang;
use io_reader::read_io_file;
use ldraw_parser::LDrawParser;
use types::{ConvertConfig, ConvertStats};

#[derive(Parser)]
#[command(name = "iooffbx", version, about = "将 .io 文件转换为 .fbx 文件\n\n用法：直接将一个或多个 .io 文件拖到本程序上即可转换")]
struct Args {
    /// 输入 .io 文件路径（可拖拽多个文件到 .exe 上）
    #[arg(required = true, num_args = 1..)]
    input: Vec<PathBuf>,
    /// 输出 .fbx 文件路径（仅单文件时有效，省略时自动生成同名 .fbx）
    #[arg(long)]
    output: Option<PathBuf>,
    /// LDraw 零件库目录路径
    #[arg(long)]
    ldraw_path: Option<PathBuf>,
    /// 使用合并模式导出（按颜色合并，默认关闭）
    #[arg(long)]
    merged: bool,
    /// 输出详细日志
    #[arg(short, long)]
    verbose: bool,
}

pub struct Converter {
    config: ConvertConfig,
}

impl Converter {
    pub fn new(config: ConvertConfig) -> Self {
        Converter { config }
    }

    pub fn run(
        &self,
        ldraw_index: std::collections::HashMap<String, PathBuf>,
        color_correction: bool,
    ) -> Result<(ConvertStats, Vec<String>), anyhow::Error> {
        let start = std::time::Instant::now();

        // 进度条样式
        let bar_style = indicatif::ProgressStyle::with_template(
            "{bar:40.cyan/blue} {pos}/{len}"
        ).unwrap().progress_chars("##-");
        let msg_style = indicatif::ProgressStyle::with_template(
            "{spinner:.cyan} {msg}"
        ).unwrap().tick_strings(&["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"]);

        let mp = indicatif::MultiProgress::new();
        let bar = mp.add(indicatif::ProgressBar::new(4)); // 4 个阶段
        let msg = mp.add(indicatif::ProgressBar::new_spinner());
        bar.set_style(bar_style);
        msg.set_style(msg_style);
        msg.enable_steady_tick(std::time::Duration::from_millis(80));

        // 2. 读取 .io 文件
        msg.set_message("读取 .io 文件");
        let package = read_io_file(&self.config.input_path)
            .map_err(|e| anyhow::anyhow!("读取 .io 文件失败: {}", e))?;
        bar.inc(1);

        // 3. 解析 LDraw MPD
        msg.set_message("解析 MPD 模型");
        let mut parser = LDrawParser::new();
        let mpd = parser
            .parse_mpd(&package.model_ldr)
            .map_err(|e| anyhow::anyhow!("解析 LDraw 文件失败: {}", e))?;
        bar.inc(1);

        // 4. 构建几何数据
        msg.set_message("展开零件...");

        let bar2 = bar.clone();
        let msg_clone = msg.clone();

        let mut builder = GeometryBuilder::new(
            &mut parser,
            &package.custom_parts,
            ldraw_index,
            self.config.verbose,
        );
        builder.set_progress_callback(Box::new(move || {
            bar2.inc(1);
            msg_clone.set_message("展开零件完成");
        }));
        let geometry = builder.build(&mpd, self.config.instancing);

        // 5. 导出 FBX
        msg.set_message("导出 FBX");
        let export_stats = match geometry.mode {
            ExportMode::Merged(ref meshes) => {
                fbx_exporter::FbxExporter::export_merged(
                    &self.config.output_path, meshes, color_correction,
                )
                .map_err(|e| anyhow::anyhow!("导出 FBX 失败: {}", e))?
            }
            ExportMode::Instanced {
                ref geometries,
                ref instances,
            } => {
                fbx_exporter::FbxExporter::export_instanced(
                    &self.config.output_path,
                    geometries,
                    instances,
                    color_correction,
                )
                .map_err(|e| anyhow::anyhow!("导出 FBX 失败: {}", e))?
            }
        };

        msg.finish_and_clear();
        bar.finish_and_clear();

        let elapsed = start.elapsed();

        let stats = ConvertStats {
            elapsed_ms: elapsed.as_millis() as u64,
            mesh_count: export_stats.mesh_count,
            vertex_count: export_stats.vertex_count,
            triangle_count: export_stats.triangle_count,
            material_count: export_stats.material_count,
        };

        Ok((stats, geometry.missing_parts))
    }
}

fn wait_for_enter(msg: &str) {
    println!();
    println!("{msg}");
    let _ = std::io::stdin().read_line(&mut String::new());
}

// ── Timestamp ────────────────────────────────────────────────────────

#[cfg(windows)]
fn local_timestamp() -> String {
    #[repr(C)]
    #[allow(non_snake_case, dead_code)]
    struct SYSTEMTIME {
        wYear: u16, wMonth: u16, wDayOfWeek: u16, wDay: u16,
        wHour: u16, wMinute: u16, wSecond: u16, wMilliseconds: u16,
    }
    unsafe extern "system" { fn GetLocalTime(lp: *mut SYSTEMTIME); }
    let mut st = SYSTEMTIME { wYear:0, wMonth:0, wDayOfWeek:0, wDay:0, wHour:0, wMinute:0, wSecond:0, wMilliseconds:0 };
    unsafe { GetLocalTime(&mut st); }
    format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond)
}
#[cfg(not(windows))]
fn local_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs().to_string()
}

fn create_history_dir(exe_dir: &Path, stem: &str, max_history: usize) -> PathBuf {
    let dir = exe_dir.join("history").join(format!("{stem}_{}", local_timestamp()));
    let _ = std::fs::create_dir_all(&dir);
    trim_history(&exe_dir.join("history"), max_history);
    dir
}

fn trim_history(history_root: &Path, max: usize) {
    if !history_root.is_dir() { return; }
    let mut dirs: Vec<_> = std::fs::read_dir(history_root)
        .into_iter().flatten().flatten()
        .filter(|e| e.path().is_dir())
        .collect();
    dirs.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
    if dirs.len() > max {
        for d in &dirs[..dirs.len() - max] {
            let _ = std::fs::remove_dir_all(d.path());
        }
    }
}

// ── Conversion job ───────────────────────────────────────────────────────

fn run_job(
    exe_dir: &Path,
    cfg: config::Config,
    input: PathBuf,
    cli_output: Option<PathBuf>,
    ldraw_path_opt: Option<PathBuf>,
    instancing: bool,
    verbose: bool,
) {
    let s: &Lang = cfg.language.strings();
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output").to_string();

    let (output, _log_dir) = if let Some(p) = cli_output {
        let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| exe_dir.to_path_buf());
        (p, dir)
    } else if let Some(ref custom) = cfg.output_dir {
        let _ = std::fs::create_dir_all(custom);
        (custom.join(format!("{stem}.fbx")), custom.clone())
    } else {
        let dir = create_history_dir(exe_dir, &stem, cfg.max_history);
        (dir.join(format!("{stem}.fbx")), dir)
    };

    println!("{}: {}", s.conv_input,  input.display());
    println!("{}: {}", s.conv_output, output.display());

    let pre_mp = indicatif::MultiProgress::new();
    let pre_bar = pre_mp.add(indicatif::ProgressBar::new(2));
    let pre_msg = pre_mp.add(indicatif::ProgressBar::new_spinner());
    pre_bar.set_style(
        indicatif::ProgressStyle::with_template("{bar:40.cyan/blue} {pos}/{len}")
            .unwrap().progress_chars("##-"),
    );
    pre_msg.set_style(
        indicatif::ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap().tick_strings(&["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"]),
    );
    pre_msg.enable_steady_tick(std::time::Duration::from_millis(80));

    pre_msg.set_message(s.conv_indexing);
    let mut color_roots: Vec<PathBuf> = Vec::new();
    if let Some(ref p) = ldraw_path_opt { color_roots.push(resolve_library_root(p)); }
    color_roots.extend(cfg.library_paths.iter().map(|p| resolve_library_root(p)));
    let fallback_ldraw = exe_dir.join("ldraw");
    if fallback_ldraw.is_dir() { color_roots.push(fallback_ldraw.clone()); }
    let _ = load_ldconfig_from_roots(&color_roots);
    pre_bar.inc(1);

    let custom_folder = exe_dir.join("custom_parts");
    if !custom_folder.exists() {
        let _ = std::fs::create_dir_all(&custom_folder);
    }

    pre_msg.set_message(s.conv_building);
    let (ldraw_index, _index_count, _index_warnings) =
        config::build_combined_index(exe_dir, &cfg, ldraw_path_opt.as_deref());
    pre_bar.inc(1);
    pre_msg.finish_and_clear();
    pre_bar.finish_and_clear();

    if cfg.library_paths.is_empty() && ldraw_path_opt.is_none() && !fallback_ldraw.is_dir() {
        println!("{}", format!("⚠  {}", s.conv_no_lib).yellow());
    }

    let color_correction = cfg.settings.color_correction;

    let convert_config = ConvertConfig {
        input_path: input,
        output_path: output.clone(),
        ldraw_path: exe_dir.join("ldraw"),
        instancing,
        verbose,
    };
    let converter = Converter::new(convert_config);

    match converter.run(ldraw_index, color_correction) {
        Ok((stats, missing_parts)) => {
            println!("────────────────────────────────────────");
            println!(
                "  │  {} {:<8} {} {:<10} {} {}",
                s.conv_mesh, stats.mesh_count,
                s.conv_vertex, stats.vertex_count,
                s.conv_tri, stats.triangle_count
            );
            println!("  │  {} {:<8} {} {}ms", s.conv_mat, stats.material_count, s.conv_time, stats.elapsed_ms);
            println!("────────────────────────────────────────");
            let unique_missing: Vec<String> = {
                let mut set = HashSet::new();
                missing_parts.into_iter().filter(|p| set.insert(p.clone())).collect()
            };
            let unknown_colors = drain_unknown_color_warnings();
            if !unknown_colors.is_empty() {
                println!("{}", format!("⚠ {}", s.conv_warn_color.replacen("{}", &unknown_colors.len().to_string(), 1)).yellow());
                for id in &unknown_colors {
                    println!("{}", format!("  [WARN] color ID {id} → #808080").yellow());
                }
                println!("────────────────────────────────────────");
            }
            if !unique_missing.is_empty() {
                println!("{}", format!("⚠ {}", s.conv_warn_missing.replacen("{}", &unique_missing.len().to_string(), 1)).bold().red());
                for part in &unique_missing {
                    println!("  {} {}", "✗".red(), part.red());
                }
            }
        }
        Err(e) => {
            eprintln!("{}", format!("❌ {}: {e}", s.conv_fail).bold().red());
        }
    };
}

fn ensure_runtime_dirs(exe_dir: &Path) {
    let _ = std::fs::create_dir_all(exe_dir.join("custom_parts"));
    let _ = std::fs::create_dir_all(exe_dir.join("history"));
}

fn main() {
    // 启用 Windows 终端 ANSI 颜色支持
    #[cfg(windows)]
    let _ = colored::control::set_virtual_terminal(true);

    let args = match Args::try_parse() {
        Ok(a) => a,
        Err(e) => {
            if std::env::args_os().count() <= 1 {
                let exe_dir = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                ensure_runtime_dirs(&exe_dir);
                let cfg = config::load_config(&exe_dir);
                use terminal_settings::SettingsResult;
                match terminal_settings::run_settings(cfg) {
                    SettingsResult::Saved(new_cfg) => {
                        config::save_config(&exe_dir, &new_cfg);
                    }
                    SettingsResult::Discarded => {}
                    SettingsResult::Convert { config: new_cfg, path } => {
                        config::save_config(&exe_dir, &new_cfg);
                        run_job(&exe_dir, new_cfg, path, None, None, true, false);
                    }
                }
            } else {
                eprintln!("{e}");
                wait_for_enter("Press Enter to close\u{2026}");
            }
            return;
        }
    };

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    ensure_runtime_dirs(&exe_dir);
    let cfg = config::load_config(&exe_dir);

    let total = args.input.len();
    for (i, input) in args.input.into_iter().enumerate() {
        if total > 1 {
            println!("[{}/{}] {}", i + 1, total, input.display());
        }
        let cli_output = if total == 1 { args.output.clone() } else { None };
        run_job(&exe_dir, cfg.clone(), input, cli_output, args.ldraw_path.clone(), !args.merged, args.verbose);
    }
    wait_for_enter(cfg.language.strings().conv_press_enter);
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use super::Args;

    #[test]
    fn test_help_contains_version() {
        let result = Args::try_parse_from(["iooffbx", "--help"]);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("iooffbx") || msg.contains("0.1"), "help should mention tool name/version");
    }

    #[test]
    fn test_missing_args_returns_error() {
        let result = Args::try_parse_from(["iooffbx"]);
        assert!(result.is_err(), "Missing required args should return an error");
    }

    #[test]
    fn test_input_only_auto_output() {
        let args = Args::try_parse_from(["iooffbx", "input.io"]).unwrap();
        assert_eq!(args.input[0].to_str().unwrap(), "input.io");
        assert!(args.output.is_none(), "output should be optional");
    }

    #[test]
    fn test_multi_input() {
        let args = Args::try_parse_from(["iooffbx", "a.io", "b.io", "c.io"]).unwrap();
        assert_eq!(args.input.len(), 3);
        assert_eq!(args.input[0].to_str().unwrap(), "a.io");
        assert_eq!(args.input[2].to_str().unwrap(), "c.io");
    }

    #[test]
    fn test_positional_args_parsed() {
        let args = Args::try_parse_from(["iooffbx", "input.io", "--output", "output.fbx"]).unwrap();
        assert_eq!(args.input[0].to_str().unwrap(), "input.io");
        assert_eq!(args.output.unwrap().to_str().unwrap(), "output.fbx");
    }

    #[test]
    fn test_ldraw_path_arg() {
        let args = Args::try_parse_from(["iooffbx", "in.io", "--ldraw-path", "/usr/share/ldraw"]).unwrap();
        assert!(args.ldraw_path.is_some());
        assert_eq!(args.ldraw_path.unwrap().to_str().unwrap(), "/usr/share/ldraw");
    }

    #[test]
    fn test_verbose_flag() {
        let args_long  = Args::try_parse_from(["iooffbx", "in.io", "--verbose"]).unwrap();
        let args_short = Args::try_parse_from(["iooffbx", "in.io", "-v"]).unwrap();
        assert!(args_long.verbose);
        assert!(args_short.verbose);
    }

    #[test]
    fn test_default_instancing_mode() {
        let args = Args::try_parse_from(["iooffbx", "in.io"]).unwrap();
        assert!(!args.merged, "Default should be instancing (merged=false)");
    }
}
