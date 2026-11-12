#![allow(dead_code)]



 use std::collections::{HashMap, HashSet};
 use std::path::PathBuf;
 use std::sync::{Mutex, OnceLock, RwLock};
 
 
 
#[derive(Debug, Clone, Copy, PartialEq)]

pub enum MaterialType {

    Normal,

    Trans,

    Chrome,

    Pearlescent,

    Metal,

    Rubber,

}



 #[derive(Debug, Clone, Copy)]
pub struct ResolvedColor {

    pub id: u32,

    pub name: &'static str,

    pub rgb: [f32; 3],

    pub alpha: f32,

    pub material_type: MaterialType,

}



 fn dynamic_colors() -> &'static RwLock<HashMap<u32, ResolvedColor>> {
 
     static DYNAMIC_COLORS: OnceLock<RwLock<HashMap<u32, ResolvedColor>>> = OnceLock::new();
 
     DYNAMIC_COLORS.get_or_init(|| RwLock::new(HashMap::new()))
 
 }
 
 
 
 fn warned_unknown_ids() -> &'static Mutex<HashSet<u32>> {
 
     static WARNED_UNKNOWN_IDS: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
 
     WARNED_UNKNOWN_IDS.get_or_init(|| Mutex::new(HashSet::new()))
 
 }

pub fn drain_unknown_color_warnings() -> Vec<u32> {
    if let Ok(mut set) = warned_unknown_ids().lock() {
        let ids: Vec<u32> = set.iter().copied().collect();
        set.clear();
        let mut sorted = ids;
        sorted.sort_unstable();
        sorted
    } else {
        vec![]
    }
}
 
 
 
 fn leak_str(value: String) -> &'static str {
 
     Box::leak(value.into_boxed_str())
 
 }
 
 
 
fn hex_to_rgb(hex: &str) -> [f32; 3] {

    // 期望格式 "#RRGGBB"

    let hex = hex.trim_start_matches('#');

    let r = hex.get(0..2).and_then(|s| u8::from_str_radix(s, 16).ok()).unwrap_or(0);

    let g = hex.get(2..4).and_then(|s| u8::from_str_radix(s, 16).ok()).unwrap_or(0);

    let b = hex.get(4..6).and_then(|s| u8::from_str_radix(s, 16).ok()).unwrap_or(0);

    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]

}



 fn parse_ldconfig_line(line: &str) -> Option<ResolvedColor> {
 
     let trimmed = line.trim();
 
     if !trimmed.starts_with("0 !COLOUR ") {
 
         return None;
 
     }
 
     let tokens: Vec<&str> = trimmed.split_whitespace().collect();
 
     if tokens.len() < 7 {
 
         return None;
 
     }
 
     let mut name = None;
     let mut id = None;
     let mut value = None;
     let mut alpha = 1.0_f32;
     let mut material_type = MaterialType::Normal;
 
     let mut i = 2usize;
     if i < tokens.len() {
 
         name = Some(tokens[i].to_string());
         i += 1;
 
     }
 
     while i < tokens.len() {
 
         match tokens[i] {
 
             "CODE" if i + 1 < tokens.len() => {
                 id = tokens[i + 1].parse::<u32>().ok();
                 i += 2;
             }
             "VALUE" if i + 1 < tokens.len() => {
                 value = Some(tokens[i + 1].to_string());
                 i += 2;
             }
             "ALPHA" if i + 1 < tokens.len() => {
                 alpha = tokens[i + 1].parse::<f32>().ok().unwrap_or(255.0) / 255.0;
                 i += 2;
             }
             "CHROME" => {
                 material_type = MaterialType::Chrome;
                 i += 1;
             }
             "PEARLESCENT" => {
                 material_type = MaterialType::Pearlescent;
                 i += 1;
             }
             "METAL" => {
                 material_type = MaterialType::Metal;
                 i += 1;
             }
             "RUBBER" => {
                 material_type = MaterialType::Rubber;
                 i += 1;
             }
             "MATERIAL" => { break; }
             _ => {
                 i += 1;
             }
 
         }
 
     }
 
     let id = id?;
     let value = value?;
     if alpha < 1.0 && material_type == MaterialType::Normal {
 
         material_type = MaterialType::Trans;
 
     }
 
     Some(ResolvedColor {
         id,
         name: leak_str(name.unwrap_or_else(|| format!("Color_{id}"))),
         rgb: hex_to_rgb(&value),
         alpha,
         material_type,
     })
 
 }
 
 
 
 pub fn load_ldconfig_from_roots(roots: &[PathBuf]) -> (usize, Vec<String>) {
 
     let mut warnings = Vec::new();
     let mut guard = match dynamic_colors().write() {
         Ok(g) => g,
         Err(_) => return (0, vec!["动态颜色表锁定失败，已跳过 LDConfig.ldr 加载".to_string()]),
     };
 
     guard.clear();
 
     for root in roots {
 
         let candidates = [root.join("LDConfig.ldr"), root.join("ldconfig.ldr")];
         let Some(path) = candidates.into_iter().find(|p| p.is_file()) else {
             continue;
         };
 
         match std::fs::read_to_string(&path) {
             Ok(content) => {
                 for line in content.lines() {
                     if let Some(color) = parse_ldconfig_line(line) {
                         guard.entry(color.id).or_insert(color);
                     }
                 }
             }
             Err(e) => warnings.push(format!("无法读取 {}: {}", path.display(), e)),
         }
 
     }
 
     let loaded = guard.len();
     (loaded, warnings)
 
 }
 
 
 
pub fn resolve_color(id: u32) -> ResolvedColor {

     if let Ok(map) = dynamic_colors().read() {
 
         if let Some(c) = map.get(&id) {
 
             return *c;
 
         }
 
     }

    // LDraw 直接色：0x2RRGGBB（不透明）或 0x3RRGGBB（半透明）

    if id >= 0x2000000 && id <= 0x3FFFFFF {

        let r = ((id >> 16) & 0xFF) as f32 / 255.0;

        let g = ((id >> 8) & 0xFF) as f32 / 255.0;

        let b = (id & 0xFF) as f32 / 255.0;

        let (alpha, material_type) = if id >= 0x3000000 {

            (128.0_f32 / 255.0, MaterialType::Trans)

        } else {

            (1.0_f32, MaterialType::Normal)

        };

        return ResolvedColor { id, name: "DirectColor", rgb: [r, g, b], alpha, material_type };

    }

    match id {

        // ── 普通色 ──────────────────────────────────────────────────

        0  => ResolvedColor { id, name: "Black",            rgb: hex_to_rgb("#1B2A34"), alpha: 1.0, material_type: MaterialType::Normal },

        1  => ResolvedColor { id, name: "Blue",             rgb: hex_to_rgb("#1E5AA8"), alpha: 1.0, material_type: MaterialType::Normal },

        2  => ResolvedColor { id, name: "Green",            rgb: hex_to_rgb("#00852B"), alpha: 1.0, material_type: MaterialType::Normal },

        3  => ResolvedColor { id, name: "Dark_Turquoise",   rgb: hex_to_rgb("#069D9F"), alpha: 1.0, material_type: MaterialType::Normal },

        4  => ResolvedColor { id, name: "Red",              rgb: hex_to_rgb("#B40000"), alpha: 1.0, material_type: MaterialType::Normal },

        5  => ResolvedColor { id, name: "Dark_Pink",        rgb: hex_to_rgb("#D3359D"), alpha: 1.0, material_type: MaterialType::Normal },

        6  => ResolvedColor { id, name: "Brown",            rgb: hex_to_rgb("#543324"), alpha: 1.0, material_type: MaterialType::Normal },

        7  => ResolvedColor { id, name: "Light_Grey",       rgb: hex_to_rgb("#8A928D"), alpha: 1.0, material_type: MaterialType::Normal },

        8  => ResolvedColor { id, name: "Dark_Grey",        rgb: hex_to_rgb("#545955"), alpha: 1.0, material_type: MaterialType::Normal },

        9  => ResolvedColor { id, name: "Light_Blue",       rgb: hex_to_rgb("#97CBD9"), alpha: 1.0, material_type: MaterialType::Normal },

        10 => ResolvedColor { id, name: "Bright_Green",     rgb: hex_to_rgb("#58AB41"), alpha: 1.0, material_type: MaterialType::Normal },

        11 => ResolvedColor { id, name: "Light_Turquoise",  rgb: hex_to_rgb("#00AAA4"), alpha: 1.0, material_type: MaterialType::Normal },

        12 => ResolvedColor { id, name: "Salmon",           rgb: hex_to_rgb("#F06D61"), alpha: 1.0, material_type: MaterialType::Normal },

        13 => ResolvedColor { id, name: "Pink",             rgb: hex_to_rgb("#F6A9BB"), alpha: 1.0, material_type: MaterialType::Normal },

        14 => ResolvedColor { id, name: "Yellow",           rgb: hex_to_rgb("#FAC80A"), alpha: 1.0, material_type: MaterialType::Normal },

        15 => ResolvedColor { id, name: "White",            rgb: hex_to_rgb("#F4F4F4"), alpha: 1.0, material_type: MaterialType::Normal },

        19 => ResolvedColor { id, name: "Tan",              rgb: hex_to_rgb("#D7BA8C"), alpha: 1.0, material_type: MaterialType::Normal },

        25 => ResolvedColor { id, name: "Orange",           rgb: hex_to_rgb("#D67923"), alpha: 1.0, material_type: MaterialType::Normal },

        26 => ResolvedColor { id, name: "Magenta",          rgb: hex_to_rgb("#901F76"), alpha: 1.0, material_type: MaterialType::Normal },

        27 => ResolvedColor { id, name: "Lime",             rgb: hex_to_rgb("#A5CA18"), alpha: 1.0, material_type: MaterialType::Normal },

        28 => ResolvedColor { id, name: "Dark_Tan",         rgb: hex_to_rgb("#897D62"), alpha: 1.0, material_type: MaterialType::Normal },

        29 => ResolvedColor { id, name: "Bright_Pink",      rgb: hex_to_rgb("#FF9ECD"), alpha: 1.0, material_type: MaterialType::Normal },

        71 => ResolvedColor { id, name: "Light_Bluish_Grey",rgb: hex_to_rgb("#969696"), alpha: 1.0, material_type: MaterialType::Normal },

        72 => ResolvedColor { id, name: "Dark_Bluish_Grey", rgb: hex_to_rgb("#646464"), alpha: 1.0, material_type: MaterialType::Normal },



        // ── 透明色 ──────────────────────────────────────────────────

        32 => ResolvedColor { id, name: "Trans_Black_IR_Lens",    rgb: hex_to_rgb("#000000"), alpha: 210.0 / 255.0, material_type: MaterialType::Trans },

        33 => ResolvedColor { id, name: "Trans_Dark_Blue",        rgb: hex_to_rgb("#0020A0"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },

        34 => ResolvedColor { id, name: "Trans_Green",            rgb: hex_to_rgb("#237841"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },

        35 => ResolvedColor { id, name: "Trans_Bright_Green",     rgb: hex_to_rgb("#56E646"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },

        36 => ResolvedColor { id, name: "Trans_Red",              rgb: hex_to_rgb("#C91A09"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },

        37 => ResolvedColor { id, name: "Trans_Dark_Pink",        rgb: hex_to_rgb("#DF6695"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },

        38 => ResolvedColor { id, name: "Trans_Neon_Orange",      rgb: hex_to_rgb("#FF800D"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },

        39 => ResolvedColor { id, name: "Trans_Very_Light_Blue",  rgb: hex_to_rgb("#C1DFF0"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },

        40 => ResolvedColor { id, name: "Trans_Brown",            rgb: hex_to_rgb("#635F52"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },

        41 => ResolvedColor { id, name: "Trans_Medium_Blue",      rgb: hex_to_rgb("#559AB7"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },

        42 => ResolvedColor { id, name: "Trans_Neon_Green",       rgb: hex_to_rgb("#C0FF00"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },

        43 => ResolvedColor { id, name: "Trans_Light_Blue",       rgb: hex_to_rgb("#AEE9EF"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },

        46 => ResolvedColor { id, name: "Trans_Yellow",           rgb: hex_to_rgb("#F5CD2F"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },

        47 => ResolvedColor { id, name: "Trans_Clear",            rgb: hex_to_rgb("#FCFCFC"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },

        52 => ResolvedColor { id, name: "Trans_Purple",           rgb: hex_to_rgb("#A5A5CB"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },

        57 => ResolvedColor { id, name: "Trans_Orange",           rgb: hex_to_rgb("#F08F1C"), alpha: 128.0 / 255.0, material_type: MaterialType::Trans },



        // ── Chrome 色 ────────────────────────────────────────────────

        60  => ResolvedColor { id, name: "Chrome_Antique_Brass", rgb: hex_to_rgb("#645A4C"), alpha: 1.0, material_type: MaterialType::Chrome },

        61  => ResolvedColor { id, name: "Chrome_Blue",          rgb: hex_to_rgb("#6C96BF"), alpha: 1.0, material_type: MaterialType::Chrome },

        62  => ResolvedColor { id, name: "Chrome_Green",         rgb: hex_to_rgb("#3CB371"), alpha: 1.0, material_type: MaterialType::Chrome },

        63  => ResolvedColor { id, name: "Chrome_Pink",          rgb: hex_to_rgb("#AA4D8E"), alpha: 1.0, material_type: MaterialType::Chrome },

        64  => ResolvedColor { id, name: "Chrome_Black",         rgb: hex_to_rgb("#1B2A34"), alpha: 1.0, material_type: MaterialType::Chrome },

        334 => ResolvedColor { id, name: "Chrome_Gold",          rgb: hex_to_rgb("#DFC176"), alpha: 1.0, material_type: MaterialType::Chrome },

        383 => ResolvedColor { id, name: "Chrome_Silver",        rgb: hex_to_rgb("#CECECE"), alpha: 1.0, material_type: MaterialType::Chrome },



        // ── Pearlescent 色 ───────────────────────────────────────────

        83  => ResolvedColor { id, name: "Pearl_Black",      rgb: hex_to_rgb("#0A1327"), alpha: 1.0, material_type: MaterialType::Pearlescent },

        134 => ResolvedColor { id, name: "Copper",           rgb: hex_to_rgb("#764D3B"), alpha: 1.0, material_type: MaterialType::Pearlescent },

        135 => ResolvedColor { id, name: "Pearl_Light_Grey", rgb: hex_to_rgb("#A0A0A0"), alpha: 1.0, material_type: MaterialType::Pearlescent },

        142 => ResolvedColor { id, name: "Pearl_Light_Gold", rgb: hex_to_rgb("#DEAC66"), alpha: 1.0, material_type: MaterialType::Pearlescent },

        147 => ResolvedColor { id, name: "Pearl_Dark_Gold",  rgb: hex_to_rgb("#83724F"), alpha: 1.0, material_type: MaterialType::Pearlescent },

        148 => ResolvedColor { id, name: "Pearl_Dark_Grey",  rgb: hex_to_rgb("#484D48"), alpha: 1.0, material_type: MaterialType::Pearlescent },

        179 => ResolvedColor { id, name: "Pearl_Silver",     rgb: hex_to_rgb("#898788"), alpha: 1.0, material_type: MaterialType::Pearlescent },

        183 => ResolvedColor { id, name: "Pearl_White",      rgb: hex_to_rgb("#F6F2DF"), alpha: 1.0, material_type: MaterialType::Pearlescent },

        297 => ResolvedColor { id, name: "Pearl_Gold",       rgb: hex_to_rgb("#AA7F2E"), alpha: 1.0, material_type: MaterialType::Pearlescent },

        315 => ResolvedColor { id, name: "Flat_Silver",      rgb: hex_to_rgb("#8C8C8C"), alpha: 1.0, material_type: MaterialType::Pearlescent },



        // ── Metal 色 ─────────────────────────────────────────────────

        80 => ResolvedColor { id, name: "Metallic_Silver", rgb: hex_to_rgb("#767676"), alpha: 1.0, material_type: MaterialType::Metal },

        82 => ResolvedColor { id, name: "Metallic_Gold",   rgb: hex_to_rgb("#DBAC34"), alpha: 1.0, material_type: MaterialType::Metal },



        // ── 未知颜色 ─────────────────────────────────────────────────

        _ => {

            if let Ok(mut warned) = warned_unknown_ids().lock() {
 
                let _ = warned.insert(id);
 
            }

            ResolvedColor {

                id,

                name: "Unknown",

                rgb: [0.502, 0.502, 0.502],

                alpha: 1.0,

                material_type: MaterialType::Normal,

            }

        }

    }

}



pub fn material_name(color: &ResolvedColor) -> String {

    if color.name == "Unknown" {

        return format!("LDraw_{}_Unknown", color.id);

    }

    let type_str = match color.material_type {

        MaterialType::Normal      => "Normal",

        MaterialType::Trans       => "Trans",

        MaterialType::Chrome      => "Chrome",

        MaterialType::Pearlescent => "Pearlescent",

        MaterialType::Metal       => "Metal",

        MaterialType::Rubber      => "Rubber",

    };

    format!("LDraw_{}_{}_{}", color.id, type_str, color.name)

}
