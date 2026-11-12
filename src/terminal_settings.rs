use std::io::{self, Write};
use std::path::PathBuf;

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};

use crate::config::Config;
use crate::i18n::Lang;

// ── Public types ─────────────────────────────────────────────────────────────

pub enum SettingsResult {
    Saved(Config),
    Discarded,
    Convert { config: Config, path: PathBuf },
}

// ── Palette ─────────────────────────────────────────────────────────────
const R:    &str = "\x1B[0m";
const DECO: &str = "\x1B[38;5;240m";
const BODY: &str = "\x1B[38;5;247m";
const BOLD: &str = "\x1B[1;38;5;15m";
const BLUE: &str = "\x1B[38;5;75m";
const WARN: &str = "\x1B[38;5;203m";
const SEP:  &str = "  ────────────────────────────────────────────────────────────";

const HIDE_CURSOR:    &str = "\x1B[?25l";
const SHOW_CURSOR:    &str = "\x1B[?25h";
const GO_HOME:        &str = "\x1B[H";
const CLEAR_EOL:      &str = "\x1B[K";
const CLEAR_BELOW:    &str = "\x1B[J";
const CLEAR_ALL:      &str = "\x1B[2J\x1B[3J";
const ALT_SCREEN_ON:  &str = "\x1B[?1049h";
const ALT_SCREEN_OFF: &str = "\x1B[?1049l";

fn clear_main_screen() {
    flush_raw(&[CLEAR_ALL, GO_HOME]);
}

// ── Main menu item indices ────────────────────────────────────────────────────
const IDX_PATH:    usize = 0;
const IDX_CC:      usize = 1;
const IDX_OUTDIR:  usize = 2;
const IDX_HISTORY: usize = 3;
const IDX_HELP:    usize = 4;
const IDX_LANG:    usize = 5;
const IDX_SAVE:    usize = 7;
const IDX_QUIT:    usize = 8;

fn make_menu_items(s: &Lang) -> Vec<&'static str> {
    vec![
        s.menu_paths,    // 0
        s.menu_color,    // 1
        s.menu_outdir,   // 2
        s.menu_history,  // 3
        s.menu_help,     // 4
        s.menu_language, // 5
        "",              // 6 spacer
        s.menu_save,     // 7
        s.menu_quit,     // 8
    ]
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run_settings(mut cfg: Config) -> SettingsResult {
    let _ = enable_raw_mode();
    flush_raw(&[ALT_SCREEN_ON, CLEAR_ALL, GO_HOME, HIDE_CURSOR]);

    let result = main_loop(&mut cfg);

    flush_raw(&[SHOW_CURSOR, ALT_SCREEN_OFF]);
    let _ = disable_raw_mode();

    result
}

// ── Main loop ─────────────────────────────────────────────────────────────────

fn main_loop(cfg: &mut Config) -> SettingsResult {
    let mut sel: usize = 0;
    let mut needs_redraw = true;
    let mut paste_buf = String::new();

    loop {
        let menu = make_menu_items(cfg.language.strings());
        let menu_len = menu.len();

        if needs_redraw {
            draw_main(cfg, sel);
            needs_redraw = false;
        }

        if let Ok(Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            ..
        })) = event::read()
        {
            match code {
                KeyCode::Char(c) => {
                    paste_buf.push(c);
                }
                KeyCode::Enter => {
                    let s = paste_buf.trim().trim_matches('"').to_string();
                    paste_buf.clear();
                    if s.to_lowercase().ends_with(".io") {
                        return SettingsResult::Convert {
                            config: cfg.clone(),
                            path: PathBuf::from(s),
                        };
                    }
                    needs_redraw = true;
                    match sel {
                        IDX_PATH    => path_editor(&mut cfg.library_paths, cfg.language.strings()),
                        IDX_CC      => { select_color_correction(cfg); }
                        IDX_OUTDIR  => inline_input_dir(cfg),
                        IDX_HISTORY => inline_input_number(cfg),
                        IDX_HELP    => show_help(cfg.language.strings()),
                        IDX_LANG    => { select_language(cfg); }
                        IDX_SAVE    => return SettingsResult::Saved(cfg.clone()),
                        IDX_QUIT    => return SettingsResult::Discarded,
                        _           => { needs_redraw = false; }
                    }
                }
                KeyCode::Up => {
                    paste_buf.clear();
                    needs_redraw = true;
                    if sel > 0 {
                        sel -= 1;
                        if menu.get(sel).map(|s| s.is_empty()).unwrap_or(false) && sel > 0 { sel -= 1; }
                    }
                }
                KeyCode::Down => {
                    paste_buf.clear();
                    needs_redraw = true;
                    if sel + 1 < menu_len {
                        sel += 1;
                        if menu.get(sel).map(|s| s.is_empty()).unwrap_or(false) && sel + 1 < menu_len { sel += 1; }
                    }
                }
                KeyCode::Esc => {
                    paste_buf.clear();
                    return SettingsResult::Discarded;
                }
                _ => { paste_buf.clear(); }
            }
        }
    }
}

// ── Language selector ─────────────────────────────────────────────────────

fn select_language(cfg: &mut Config) {
    use crate::i18n::Language;
    let options = [
        Language::Zh, Language::ZhTw,
        Language::En, Language::Fr, Language::It, Language::De, Language::Es,
        Language::Ja, Language::Ko, Language::Ru, Language::PtBr, Language::Pl, Language::Tr,
    ];
    let mut sel: usize = options.iter().position(|l| l == &cfg.language).unwrap_or(0);

    loop {
        let s = cfg.language.strings();
        let mut b = Buf::new();
        b.home();
        b.empty();
        b.ln(&format!("  {BOLD}iooffbx{R}  {DECO}|  {}{R}", s.menu_language));
        b.empty();
        b.ln(&format!("{DECO}{SEP}{R}"));
        b.empty();
        for (i, lang) in options.iter().enumerate() {
            let label = lang.label();
            if i == sel {
                b.ln(&format!("  {BLUE}▶{R}  {BOLD}{label}{R}"));
            } else {
                b.ln(&format!("     {BODY}{label}{R}"));
            }
        }
        b.empty();
        b.ln(&format!("{DECO}{SEP}{R}"));
        b.ln(&format!("  {BODY}[↑↓] {R}{DECO}移动 / Move{R}  {BODY}[Enter] {R}{DECO}确认 / Confirm{R}  {BODY}[Esc] {R}{DECO}取消 / Cancel{R}"));
        b.clear_below();
        b.flush();

        match event::read() {
            Ok(Event::Key(KeyEvent { code, kind: KeyEventKind::Press, .. })) => match code {
                KeyCode::Up   => { if sel > 0 { sel -= 1; } }
                KeyCode::Down => { if sel + 1 < options.len() { sel += 1; } }
                KeyCode::Enter => {
                    cfg.language = options[sel].clone();
                    break;
                }
                KeyCode::Esc => break,
                _ => {}
            },
            _ => {}
        }
    }
}

// ── Color correction selector ─────────────────────────────────────────────

fn select_color_correction(cfg: &mut Config) {
    // 0 = 线性 sRGB（开启），1 = 原始 sRGB（关闭）
    let mut sel: usize = if cfg.settings.color_correction { 0 } else { 1 };

    loop {
        let s = cfg.language.strings();
        let mut b = Buf::new();
        b.home();
        b.empty();
        b.ln(&format!("  {BOLD}iooffbx{R}  {DECO}|  {}{R}", s.menu_color));
        b.empty();
        b.ln(&format!("{DECO}{SEP}{R}"));
        b.empty();
        b.ln(&format!("  {DECO}{}{R}", s.cc_desc_h));
        b.empty();
        b.ln(&format!("  {BODY}{}{R}", s.cc_desc_1));
        b.ln(&format!("  {BODY}{}{R}", s.cc_desc_2));
        b.empty();
        b.ln(&format!("{DECO}{SEP}{R}"));
        b.empty();

        // 选项 0：线性 sRGB
        if sel == 0 {
            b.ln(&format!("  {BLUE}▶{R}  {BOLD}{} {BLUE}✓{R}", s.cc_linear));
        } else {
            b.ln(&format!("     {BODY}{}{R}", s.cc_linear));
        }
        // 选项 1：原始 sRGB
        if sel == 1 {
            b.ln(&format!("  {BLUE}▶{R}  {BOLD}{} {BLUE}✓{R}", s.cc_raw));
        } else {
            b.ln(&format!("     {BODY}{}{R}", s.cc_raw));
        }

        b.empty();
        b.ln(&format!("{DECO}{SEP}{R}"));
        b.ln(&format!("  {BODY}[↑↓]{R} {DECO}{}{R}  {BODY}[Enter]{R} {DECO}{}{R}  {BODY}[Esc]{R} {DECO}{}{R}",
            s.cc_hint_move, s.cc_hint_confirm, s.cc_hint_cancel));
        b.clear_below();
        b.flush();

        match event::read() {
            Ok(Event::Key(KeyEvent { code, kind: KeyEventKind::Press, .. })) => match code {
                KeyCode::Up   => { if sel > 0 { sel -= 1; } }
                KeyCode::Down => { if sel < 1 { sel += 1; } }
                KeyCode::Enter => {
                    cfg.settings.color_correction = sel == 0;
                    break;
                }
                KeyCode::Esc => break,
                _ => {}
            },
            _ => {}
        }
    }
}

// ── Inline input helpers ──────────────────────────────────────────────────

fn inline_input_dir(cfg: &mut Config) {
    let s = cfg.language.strings();
    flush_raw(&[ALT_SCREEN_OFF, SHOW_CURSOR]);
    let _ = disable_raw_mode();
    clear_main_screen();
    println!();
    println!("  {BOLD}iooffbx{R}  {DECO}|  {}{R}", s.outdir_title);
    println!();
    println!("{DECO}{SEP}{R}");
    println!();
    match &cfg.output_dir {
        Some(p) => {
            println!("  {BODY}{}{R}  {BOLD}{}{R}", s.outdir_current, p.display());
            println!("  {BODY}{}{R}", s.outdir_prompt_clr);
        }
        None => {
            println!("  {BODY}{DECO}{}{R}", s.outdir_default);
            println!("  {BODY}{}{R}", s.outdir_prompt_set);
        }
    }
    println!();
    print!("  {DECO}{}{R}  ", s.outdir_prompt_arr);
    let _ = io::stdout().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        let v = input.trim().to_string();
        cfg.output_dir = if v.is_empty() { None } else { Some(PathBuf::from(v)) };
    }
    let _ = enable_raw_mode();
    flush_raw(&[ALT_SCREEN_ON, HIDE_CURSOR, CLEAR_ALL, GO_HOME]);
}

fn inline_input_number(cfg: &mut Config) {
    let s = cfg.language.strings();
    flush_raw(&[ALT_SCREEN_OFF, SHOW_CURSOR]);
    let _ = disable_raw_mode();
    clear_main_screen();
    println!();
    println!("  {BOLD}iooffbx{R}  {DECO}|  {}{R}", s.hist_title);
    println!();
    println!("{DECO}{SEP}{R}");
    println!();
    println!("  {BODY}{}{R}  {BOLD}{}{R}", s.hist_current, cfg.max_history);
    println!("  {BODY}{}{R}", s.hist_prompt);
    println!();
    print!("  {DECO}›{R}  ");
    let _ = io::stdout().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        let v = input.trim();
        if let Ok(n) = v.parse::<usize>() {
            if n >= 1 { cfg.max_history = n; }
        }
    }
    let _ = enable_raw_mode();
    flush_raw(&[ALT_SCREEN_ON, HIDE_CURSOR, CLEAR_ALL, GO_HOME]);
}

fn show_help(s: &Lang) {
    let lines: Vec<String> = vec![
        format!("  {BOLD}iooffbx{R}  {DECO}|  {}{R}", s.help_title),
        format!("{DECO}{SEP}{R}"),
        String::new(),
        format!("  {DECO}{}{R}", s.help_usage_h),
        String::new(),
        format!("  {BODY}{}{R}", s.help_usage_1),
        String::new(),
        format!("  {DECO}{}{R}", s.help_dir_h),
        String::new(),
        format!("  {BODY}{}{R}", s.help_exe),
        format!("  {BODY}{}{R}", s.help_exe_desc),
        String::new(),
        format!("  {BODY}{}{R}", s.help_cfg),
        format!("  {BODY}{}{R}", s.help_cfg_desc1),
        format!("  {BODY}{}{R}", s.help_cfg_desc2),
        String::new(),
        format!("  {BODY}{}{R}", s.help_custom),
        format!("  {BODY}{}{R}", s.help_custom_desc1),
        format!("  {BODY}{}{R}", s.help_custom_desc2),
        String::new(),
        format!("  {BODY}{}{R}", s.help_hist),
        format!("  {BODY}{}{R}", s.help_hist_desc1),
        format!("  {BODY}{}{R}", s.help_hist_desc2),
        format!("  {BODY}{}{R}", s.help_hist_desc3),
        format!("  {BODY}{}{R}", s.help_hist_desc4),
        String::new(),
        format!("  {DECO}{}{R}", s.help_ldraw_h),
        String::new(),
        format!("  {BODY}{}{R}", s.help_ldraw_1),
        format!("  {BODY}{}{R}", s.help_ldraw_2),
        format!("  {BODY}{}{R}", s.help_ldraw_3),
    ];

    // 获取终端高度，留 2 行给底部状态栏
    let term_h = crossterm::terminal::size().map(|(_, h)| h as usize).unwrap_or(24);
    let view_h = term_h.saturating_sub(2);
    let total = lines.len();
    let max_scroll = total.saturating_sub(view_h);
    let mut scroll: usize = 0;

    let draw = |scroll: usize| {
        let mut b = Buf::new();
        b.home();
        for line in lines.iter().skip(scroll).take(view_h) {
            b.ln(line);
        }
        b.clear_below();
        // 底部状态栏
        let status = if max_scroll == 0 {
            format!("  {BODY}{}{R}", s.help_return)
        } else {
            format!("  {BODY}[↑↓]  {}  ({}/{}){R}", s.help_return, scroll + 1, max_scroll + 1)
        };
        b.ln(&status);
        b.flush();
    };

    draw(scroll);

    loop {
        match event::read() {
            Ok(event::Event::Key(event::KeyEvent { code, kind: event::KeyEventKind::Press, .. })) => {
                match code {
                    event::KeyCode::Enter | event::KeyCode::Esc => break,
                    event::KeyCode::Up | event::KeyCode::Char('k') => {
                        if scroll > 0 { scroll -= 1; draw(scroll); }
                    }
                    event::KeyCode::Down | event::KeyCode::Char('j') => {
                        if scroll < max_scroll { scroll += 1; draw(scroll); }
                    }
                    event::KeyCode::PageUp => {
                        scroll = scroll.saturating_sub(view_h); draw(scroll);
                    }
                    event::KeyCode::PageDown => {
                        scroll = (scroll + view_h).min(max_scroll); draw(scroll);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

// ── Main screen ────────────────────────────────────────────────────────────

fn draw_main(cfg: &Config, sel: usize) {
    let s = cfg.language.strings();
    let menu = make_menu_items(s);
    let mut b = Buf::new();
    b.home();

    b.empty();
    b.ln(&format!("  {BOLD}iooffbx{R}"));
    b.empty();
    b.ln(&format!("  {DECO}{}{R}", s.status_header));
    b.empty();

    if cfg.library_paths.is_empty() {
        b.ln(&format!("  {DECO}│{R}  {BODY}{}   {WARN}{}{R}", s.status_paths, s.status_paths_none));
    } else {
        for (i, p) in cfg.library_paths.iter().enumerate() {
            let (mark, mc) = if p.exists() { ("✓", BLUE) } else { ("✗", WARN) };
            let ps = p.to_string_lossy();
            if i == 0 {
                b.ln(&format!("  {DECO}│{R}  {BODY}{}   {BOLD}{ps}{R}  {mc}[ {mark} ]{R}", s.status_paths));
            } else {
                b.ln(&format!("  {DECO}│{R}           {BOLD}{ps}{R}  {mc}[ {mark} ]{R}"));
            }
        }
    }

    let (cc_s, cc_c) = if cfg.settings.color_correction {
        (s.status_color_on, BLUE)
    } else {
        (s.status_color_off, BODY)
    };
    b.ln(&format!("  {DECO}│{R}  {BODY}{}  {cc_c}{cc_s}{R}", s.status_color));

    let outdir_s = match &cfg.output_dir {
        Some(p) => p.to_string_lossy().into_owned(),
        None    => s.status_outdir_def.to_string(),
    };
    b.ln(&format!("  {DECO}│{R}  {BODY}{}  {BOLD}{outdir_s}{R}", s.status_outdir));
    b.ln(&format!("  {DECO}│{R}  {BODY}{}  {BOLD}{}{R}", s.status_history, cfg.max_history));

    b.empty();
    b.ln(&format!("{DECO}{SEP}{R}"));
    b.empty();

    for (i, item) in menu.iter().enumerate() {
        if item.is_empty() {
            b.ln(&format!("  {DECO}·{R}"));
        } else if i == sel {
            b.ln(&format!("  {BLUE}▶{R}  {BOLD}{item}{R}"));
        } else {
            b.ln(&format!("     {BODY}{item}{R}"));
        }
    }

    b.empty();
    b.ln(&format!("  {BODY}{}{R}", s.hint_nav));
    b.clear_below();
    b.flush();
}

// ── Path editor: add / delete / edit with undo-redo ──────────────────────────
//
// sel indices (n = current path count):
//   0 .. n-1   → existing paths
//   n          → [+ 添加路径]
//   n+1        → spacer  (never selected)
//   n+2        → 取消
//   n+3        → 保存

fn path_editor(paths: &mut Vec<PathBuf>, s: &Lang) {
    // snapshot-based undo/redo history
    let mut history: Vec<Vec<PathBuf>> = vec![paths.clone()];
    let mut hidx: usize = 0;
    let mut sel: usize = 0;

    loop {
        let n = history[hidx].len();

        // clamp / fix sel so it never lands on the spacer
        if sel == n + 1                { sel = n; }
        if sel > n + 3                 { sel = n + 3; }

        draw_path_editor(&history[hidx], sel, hidx > 0, hidx + 1 < history.len(), s);

        if let Ok(Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            modifiers,
            ..
        })) = event::read()
        {
            let n = history[hidx].len();

            match code {
                // ── navigation ──
                KeyCode::Up => {
                    if sel == n + 2 { sel = n; }           // jump over spacer
                    else if sel > 0 { sel -= 1; }
                }
                KeyCode::Down => {
                    if sel == n     { sel = n + 2; }        // jump over spacer
                    else if sel < n + 3 { sel += 1; }
                }

                // ── actions ──
                KeyCode::Enter => {
                    if sel < n {
                        let old = history[hidx][sel].clone();
                        if let Some(new_p) = input_path(s, Some(&old)) {
                            if new_p != old {
                                let mut nv = history[hidx].clone();
                                nv[sel] = new_p;
                                push_hist(&mut history, &mut hidx, nv);
                            }
                        }
                    } else if sel == n {
                        if let Some(new_p) = input_path(s, None) {
                            let mut nv = history[hidx].clone();
                            nv.push(new_p);
                            let new_sel = nv.len() - 1;
                            push_hist(&mut history, &mut hidx, nv);
                            sel = new_sel;
                        }
                    } else if sel == n + 2 {
                        return;
                    } else if sel == n + 3 {
                        *paths = history[hidx].clone();
                        return;
                    }
                }

                KeyCode::Backspace | KeyCode::Delete => {
                    if sel < n {
                        let mut nv = history[hidx].clone();
                        nv.remove(sel);
                        let new_n = nv.len();
                        push_hist(&mut history, &mut hidx, nv);
                        if sel > 0 && sel >= new_n { sel -= 1; }
                    }
                }

                // ── undo / redo ──
                KeyCode::Char('z') if modifiers.contains(KeyModifiers::CONTROL) => {
                    if hidx > 0 { hidx -= 1; }
                }
                KeyCode::Char('y') if modifiers.contains(KeyModifiers::CONTROL) => {
                    if hidx + 1 < history.len() { hidx += 1; }
                }

                KeyCode::Esc => return,
                _ => {}
            }
        }
    }
}

fn push_hist(history: &mut Vec<Vec<PathBuf>>, hidx: &mut usize, new_state: Vec<PathBuf>) {
    history.truncate(*hidx + 1);
    history.push(new_state);
    *hidx += 1;
}

fn draw_path_editor(paths: &[PathBuf], sel: usize, can_undo: bool, can_redo: bool, s: &Lang) {
    let n = paths.len();
    let mut b = Buf::new();
    b.home();

    b.ln(&format!("  {BOLD}iooffbx{R}  {DECO}|  {}{R}", s.pe_title));
    b.empty();
    b.ln(&format!("{DECO}{SEP}{R}"));
    b.empty();

    if paths.is_empty() {
        b.ln(&format!("  {DECO}{}{R}", s.pe_empty));
    }
    for (i, p) in paths.iter().enumerate() {
        let (icon, ic) = if p.exists() {
            (format!("{BLUE}✓{R}"), BLUE)
        } else {
            (format!("{WARN}✗{R}"), WARN)
        };
        let ps = p.to_string_lossy();
        if i == sel {
            b.ln(&format!("  {BLUE}▶{R}  {icon}  {BOLD}{ps}{R}"));
        } else {
            b.ln(&format!("     {icon}  {ic}{ps}{R}"));
        }
    }

    if sel == n {
        b.ln(&format!("  {BLUE}▶{R}  {BOLD}[+ {}]{R}", s.pe_add));
    } else {
        b.ln(&format!("     {DECO}[+ {}]{R}", s.pe_add));
    }

    b.ln(&format!("  {DECO}·{R}"));

    if sel == n + 2 {
        b.ln(&format!("  {BLUE}▶{R}  {BOLD}{}{R}", s.pe_cancel));
    } else {
        b.ln(&format!("     {BODY}{}{R}", s.pe_cancel));
    }

    if sel == n + 3 {
        b.ln(&format!("  {BLUE}▶{R}  {BOLD}{}{R}", s.pe_save));
    } else {
        b.ln(&format!("     {BODY}{}{R}", s.pe_save));
    }

    b.empty();
    b.ln(&format!("{DECO}{SEP}{R}"));

    let uz = if can_undo { BODY } else { DECO };
    let uy = if can_redo { BODY } else { DECO };
    b.ln(&format!(
        "  {BODY}[↑↓]{R}  {BODY}[Enter]{R}  \
         {BODY}[Bksp]{R}  {uz}[Ctrl+Z]{R} {}  {uy}[Ctrl+Y]{R} {}",
        s.pe_undo, s.pe_redo
    ));

    b.clear_below();
    b.flush();
}

// ── Text input helper (temporarily leaves raw mode) ──────────────────────────

fn input_path(s: &Lang, existing: Option<&PathBuf>) -> Option<PathBuf> {
    let title = if existing.is_some() { s.path_prompt_keep } else { s.path_prompt_new };
    flush_raw(&[ALT_SCREEN_OFF, SHOW_CURSOR]);
    let _ = disable_raw_mode();
    clear_main_screen();

    println!();
    println!("  {BOLD}iooffbx{R}  {DECO}|  {}{R}", s.pe_title);
    println!();
    println!("{DECO}{SEP}{R}");
    println!();

    if let Some(p) = existing {
        println!("  {BODY}{}{R}  {BOLD}{}{R}", s.path_current, p.to_string_lossy());
        println!("  {BODY}{}{R}", title);
    } else {
        println!("  {BODY}{}{R}", title);
    }
    println!();
    print!("  {DECO}›{R}  ");
    let _ = io::stdout().flush();

    let mut input = String::new();
    let result = match io::stdin().read_line(&mut input) {
        Ok(_) => {
            let v = input.trim().to_string();
            if v.is_empty() {
                existing.cloned()
            } else {
                let p = PathBuf::from(&v);
                if !p.exists() {
                    println!();
                    println!("  {WARN}⚠  {}{R}", s.path_invalid);
                    println!();
                    print!("  {DECO}›{R}  ");
                    let _ = io::stdout().flush();
                    let mut tmp = String::new();
                    let _ = io::stdin().read_line(&mut tmp);
                    if tmp.trim().to_lowercase() != s.path_confirm {
                        let _ = enable_raw_mode();
                        flush_raw(&[ALT_SCREEN_ON, HIDE_CURSOR, CLEAR_ALL, GO_HOME]);
                        return None;
                    }
                }
                Some(p)
            }
        }
        Err(_) => existing.cloned(),
    };

    let _ = enable_raw_mode();
    flush_raw(&[ALT_SCREEN_ON, HIDE_CURSOR, CLEAR_ALL, GO_HOME]);
    result
}

// ── Buffered output (overwrite-in-place, no flicker) ─────────────────────────

struct Buf(Vec<u8>);

impl Buf {
    fn new() -> Self { Buf(Vec::with_capacity(4096)) }

    fn home(&mut self) {
        self.0.extend_from_slice(GO_HOME.as_bytes());
    }

    fn ln(&mut self, content: &str) {
        self.0.extend_from_slice(content.as_bytes());
        self.0.extend_from_slice(CLEAR_EOL.as_bytes());
        self.0.extend_from_slice(b"\r\n");
    }

    fn empty(&mut self) {
        self.0.extend_from_slice(CLEAR_EOL.as_bytes());
        self.0.extend_from_slice(b"\r\n");
    }

    fn clear_below(&mut self) {
        self.0.extend_from_slice(CLEAR_BELOW.as_bytes());
    }

    fn flush(self) {
        let mut o = io::stdout();
        o.write_all(&self.0).unwrap();
        o.flush().unwrap();
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn flush_raw(parts: &[&str]) {
    let mut o = io::stdout();
    for s in parts { o.write_all(s.as_bytes()).unwrap(); }
    o.flush().unwrap();
}
