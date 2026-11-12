#![allow(dead_code)]



use glam::{DMat4, DVec3, DVec4};

use std::collections::HashMap;

use std::sync::Arc;



#[derive(Debug, Clone)]

pub struct Type1Ref {

    pub color: u32,

    pub transform: DMat4,

    pub file: String,

}



#[derive(Debug, Clone)]

pub struct Triangle {

    pub color: u32,

    pub vertices: [DVec3; 3],

}



#[derive(Debug, Clone)]

pub struct Quad {

    pub color: u32,

    pub vertices: [DVec3; 4],

}



#[derive(Debug, Clone, PartialEq)]

pub enum BfcMode {

    CertifyCcw,

    CertifyCw,

    NoCertify,

}



#[derive(Debug, Clone)]

pub enum LDrawLine {

    Type1(Type1Ref),

    Type3(Triangle),

    Type4(Quad),

    InvertNext,

    Other,

}



#[derive(Debug, Clone)]

pub struct SubModel {

    pub name: String,

    pub bfc_mode: BfcMode,

    pub lines: Vec<LDrawLine>,

}



#[derive(Debug, Clone)]

pub struct ParsedFile {

    pub submodels: Vec<SubModel>,

}



#[derive(Debug, thiserror::Error)]

pub enum ParseError {

    #[error("行 {line_no} 字段数量不足: {content}")]

    InvalidLine { line_no: usize, content: String },

    #[error("浮点数解析失败: {0}")]

    FloatParse(#[from] std::num::ParseFloatError),

}



pub struct LDrawParser {

    cache: HashMap<String, Arc<ParsedFile>>,

}



impl LDrawParser {

    pub fn new() -> Self {

        LDrawParser {

            cache: HashMap::new(),

        }

    }



    pub fn parse_mpd(&mut self, content: &str) -> Result<ParsedFile, ParseError> {

        let mut submodels: Vec<SubModel> = Vec::new();

        let mut current_name = "root".to_string();

        let mut current_bfc = BfcMode::NoCertify;

        let mut current_lines: Vec<LDrawLine> = Vec::new();



        for (line_no, raw_line) in content.lines().enumerate() {

            let trimmed = raw_line.trim();



            if trimmed.starts_with("0 FILE ") {

                // Push current submodel only if it has content (skip empty root placeholder)

                if !current_lines.is_empty() {

                    submodels.push(SubModel {

                        name: current_name.clone(),

                        bfc_mode: current_bfc.clone(),

                        lines: current_lines,

                    });

                }

                current_name = trimmed["0 FILE ".len()..].trim().to_string();

                current_bfc = BfcMode::NoCertify;

                current_lines = Vec::new();

            } else if trimmed == "0 NOFILE" {

                submodels.push(SubModel {

                    name: current_name.clone(),

                    bfc_mode: current_bfc.clone(),

                    lines: current_lines,

                });

                current_name = String::new();

                current_bfc = BfcMode::NoCertify;

                current_lines = Vec::new();

            } else {

                match parse_line(trimmed, line_no + 1, &mut current_bfc) {

                    Ok(Some(line)) => current_lines.push(line),

                    Ok(None) => {}

                    Err(e) => {

                        eprintln!("[WARN] 第 {} 行解析失败，已跳过: {}", line_no + 1, e);

                    }

                }

            }

        }



        // Push the last submodel

        if !current_lines.is_empty() {

            submodels.push(SubModel {

                name: current_name,

                bfc_mode: current_bfc,

                lines: current_lines,

            });

        }



        Ok(ParsedFile { submodels })

    }



    pub fn parse_file(&mut self, name: &str, content: &str) -> Result<Arc<ParsedFile>, ParseError> {

        let key = name.to_lowercase();

        if let Some(cached) = self.cache.get(&key) {

            return Ok(Arc::clone(cached));

        }

        let parsed = self.parse_mpd(content)?;

        let arc = Arc::new(parsed);

        self.cache.insert(key, Arc::clone(&arc));

        Ok(arc)

    }

}



/// Parse a single trimmed line. Returns Ok(None) for empty/blank lines.

/// Updates bfc_mode in place for BFC CERTIFY/NOCERTIFY directives.

fn parse_line(

    trimmed: &str,

    line_no: usize,

    bfc_mode: &mut BfcMode,

) -> Result<Option<LDrawLine>, ParseError> {

    if trimmed.is_empty() {

        return Ok(None);

    }



    let fields: Vec<&str> = trimmed.split_whitespace().collect();

    if fields.is_empty() {

        return Ok(None);

    }



    match fields[0] {

        "0" => {

            // BFC directives

            if trimmed.contains("BFC CERTIFY CCW") {

                *bfc_mode = BfcMode::CertifyCcw;

            } else if trimmed.contains("BFC CERTIFY CW") {

                *bfc_mode = BfcMode::CertifyCw;

            } else if trimmed.contains("BFC CERTIFY") {

                *bfc_mode = BfcMode::CertifyCcw;

            } else if trimmed.contains("BFC NOCERTIFY") {

                *bfc_mode = BfcMode::NoCertify;

            } else if trimmed.contains("BFC INVERTNEXT") {

                return Ok(Some(LDrawLine::InvertNext));

            }

            Ok(Some(LDrawLine::Other))

        }

        "1" => {

            // type1: 1 color x y z a b c d e f g h i filename

            // minimum 15 fields (index 0..14), filename may have spaces

            if fields.len() < 15 {

                eprintln!(

                    "[WARN] 第 {} 行字段数量不足，已跳过: \"{}\"",

                    line_no, trimmed

                );

                return Ok(None);

            }

            let color = parse_color(fields[1]);

            let x: f64 = fields[2].parse()?;

            let y: f64 = fields[3].parse()?;

            let z: f64 = fields[4].parse()?;

            let a: f64 = fields[5].parse()?;

            let b: f64 = fields[6].parse()?;

            let c: f64 = fields[7].parse()?;

            let d: f64 = fields[8].parse()?;

            let e: f64 = fields[9].parse()?;

            let f: f64 = fields[10].parse()?;

            let g: f64 = fields[11].parse()?;

            let h: f64 = fields[12].parse()?;

            let i: f64 = fields[13].parse()?;

            // filename is everything from field 14 onward (may contain spaces)

            let file = fields[14..].join(" ");



            // Build column-major DMat4

            // LDraw matrix row-major:

            // | a b c x |

            // | d e f y |

            // | g h i z |

            // | 0 0 0 1 |

            // glam DMat4::from_cols takes columns:

            // col0 = (a, d, g, 0), col1 = (b, e, h, 0), col2 = (c, f, i, 0), col3 = (x, y, z, 1)

            let transform = DMat4::from_cols(

                DVec4::new(a, d, g, 0.0),

                DVec4::new(b, e, h, 0.0),

                DVec4::new(c, f, i, 0.0),

                DVec4::new(x, y, z, 1.0),

            );



            Ok(Some(LDrawLine::Type1(Type1Ref {

                color,

                transform,

                file,

            })))

        }

        "3" => {

            // type3: 3 color x1 y1 z1 x2 y2 z2 x3 y3 z3 (11 fields)

            if fields.len() < 11 {

                eprintln!(

                    "[WARN] 第 {} 行字段数量不足，已跳过: \"{}\"",

                    line_no, trimmed

                );

                return Ok(None);

            }

            let color = parse_color(fields[1]);

            let v0 = DVec3::new(

                fields[2].parse()?,

                fields[3].parse()?,

                fields[4].parse()?,

            );

            let v1 = DVec3::new(

                fields[5].parse()?,

                fields[6].parse()?,

                fields[7].parse()?,

            );

            let v2 = DVec3::new(

                fields[8].parse()?,

                fields[9].parse()?,

                fields[10].parse()?,

            );

            Ok(Some(LDrawLine::Type3(Triangle {

                color,

                vertices: [v0, v1, v2],

            })))

        }

        "4" => {

            // type4: 4 color x1 y1 z1 x2 y2 z2 x3 y3 z3 x4 y4 z4 (14 fields)

            if fields.len() < 14 {

                eprintln!(

                    "[WARN] 第 {} 行字段数量不足，已跳过: \"{}\"",

                    line_no, trimmed

                );

                return Ok(None);

            }

            let color = parse_color(fields[1]);

            let v0 = DVec3::new(

                fields[2].parse()?,

                fields[3].parse()?,

                fields[4].parse()?,

            );

            let v1 = DVec3::new(

                fields[5].parse()?,

                fields[6].parse()?,

                fields[7].parse()?,

            );

            let v2 = DVec3::new(

                fields[8].parse()?,

                fields[9].parse()?,

                fields[10].parse()?,

            );

            let v3 = DVec3::new(

                fields[11].parse()?,

                fields[12].parse()?,

                fields[13].parse()?,

            );

            Ok(Some(LDrawLine::Type4(Quad {

                color,

                vertices: [v0, v1, v2, v3],

            })))

        }

        "2" | "5" => Ok(Some(LDrawLine::Other)),

        _ => Ok(Some(LDrawLine::Other)),

    }

}



/// Parse a color field: LDraw colors can be -1 (inherit), 16 (inherit), positive decimal integers,
/// or 0x2RRGGBB / 0x3RRGGBB hex strings for direct colors.

fn parse_color(s: &str) -> u32 {

    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {

        return u32::from_str_radix(hex, 16).unwrap_or(16);

    }

    let c = s.parse::<i32>().unwrap_or(16);

    if c < 0 { 16 } else { c as u32 }

}

