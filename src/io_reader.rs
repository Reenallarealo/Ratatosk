#![allow(dead_code)]



use std::collections::HashMap;

use std::fs::File;

use std::io::Read;

use std::path::{Path, PathBuf};



pub struct IoPackage {

    pub model_ldr: String,

    pub custom_parts: HashMap<String, String>,

}



#[derive(Debug, thiserror::Error)]

pub enum IoError {

    #[error("文件不存在: {0}")]

    FileNotFound(PathBuf),

    #[error("非 ZIP 格式: {0}")]

    InvalidZip(#[from] zip::result::ZipError),

    #[error("IO 错误: {0}")]

    IoError(#[from] std::io::Error),

    #[error("ZIP 中没有 model.ldr")]

    ModelLdrNotFound,

}



fn strip_bom(s: String) -> String {

    if s.starts_with('\u{FEFF}') {

        s['\u{FEFF}'.len_utf8()..].to_string()

    } else {

        s

    }

}



pub fn read_io_file(path: &Path) -> Result<IoPackage, IoError> {

    if !path.exists() {

        return Err(IoError::FileNotFound(path.to_path_buf()));

    }



    let file = File::open(path)?;

    let mut archive = zip::ZipArchive::new(file)?;



    // Find model.ldr (case-insensitive)

    let model_ldr_index = (0..archive.len()).find(|&i| {

        archive

            .by_index(i)

            .map(|e| e.name().to_lowercase() == "model.ldr")

            .unwrap_or(false)

    });



    let model_ldr_index = model_ldr_index.ok_or(IoError::ModelLdrNotFound)?;



    let model_ldr = {

        let mut entry = archive.by_index(model_ldr_index)?;

        let mut content = String::new();

        entry.read_to_string(&mut content)?;

        strip_bom(content)

    };



    // Collect CustomParts/*.dat entries

    let mut custom_parts = HashMap::new();

    for i in 0..archive.len() {

        let mut entry = archive.by_index(i)?;

        let name = entry.name().to_string();

        let name_lower = name.to_lowercase();



        if name_lower.contains("customparts/") && (name_lower.ends_with(".dat") || name_lower.ends_with(".ldr")) {

            let file_name = Path::new(&name)

                .file_name()

                .and_then(|n| n.to_str())

                .map(|n| n.to_lowercase());



            if let Some(key) = file_name {

                let mut content = String::new();

                entry.read_to_string(&mut content)?;

                custom_parts.insert(key, strip_bom(content));

            }

        }

    }



    Ok(IoPackage {

        model_ldr,

        custom_parts,

    })

}

