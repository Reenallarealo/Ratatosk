use std::path::PathBuf;

pub struct ConvertConfig {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub ldraw_path: PathBuf,
    pub instancing: bool,
    pub verbose: bool,
}

pub struct ConvertStats {
    pub elapsed_ms: u64,
    pub mesh_count: usize,
    pub vertex_count: usize,
    pub triangle_count: usize,
    pub material_count: usize,
}

pub struct ExportStats {
    pub mesh_count: usize,
    pub vertex_count: usize,
    pub triangle_count: usize,
    pub material_count: usize,
    pub normal_material_count: usize,
    pub trans_material_count: usize,
    pub chrome_material_count: usize,
    pub metal_pearl_material_count: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("写出文件失败: {0}")]
    WriteError(#[from] std::io::Error),
}
