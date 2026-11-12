#![allow(dead_code)]



use glam::{DMat4, DVec3, Mat4};

use std::collections::HashMap;

use std::path::PathBuf;



use crate::ldraw_parser::{BfcMode, LDrawLine, LDrawParser, ParsedFile, SubModel};



const LDRAW_SCALE: f64 = 0.01;



pub struct MergedMesh {

    pub color_id: u32,

    pub vertices: Vec<[f32; 3]>,

    pub triangles: Vec<[u32; 3]>,

}



pub struct PartGeometry {

    pub part_name: String,

    pub vertices: Vec<[f32; 3]>,

    pub triangles: Vec<[u32; 3]>,

}



pub struct PartInstance {

    pub part_name: String,

    pub color_id: u32,

    pub transform: Mat4,

}



pub enum ExportMode {

    Merged(Vec<MergedMesh>),

    Instanced {

        geometries: HashMap<String, PartGeometry>,

        instances: Vec<PartInstance>,

    },

}



pub struct GeometryOutput {

    pub mode: ExportMode,

    pub missing_parts: Vec<String>,

}



pub struct GeometryBuilder<'a> {

    parser: &'a mut LDrawParser,

    custom_parts: &'a HashMap<String, String>,

    ldraw_index: HashMap<String, PathBuf>,

    verbose: bool,

    progress_callback: Option<Box<dyn FnMut() + Send>>,

}



impl<'a> GeometryBuilder<'a> {

    pub fn new(

        parser: &'a mut LDrawParser,

        custom_parts: &'a HashMap<String, String>,

        ldraw_index: HashMap<String, PathBuf>,

        verbose: bool,

    ) -> Self {

        GeometryBuilder {

            parser,

            custom_parts,

            ldraw_index,

            verbose,

            progress_callback: None,

        }

    }



    pub fn set_progress_callback(&mut self, cb: Box<dyn FnMut() + Send>) {
        self.progress_callback = Some(cb);
    }

    pub fn build(&mut self, mpd: &ParsedFile, instancing: bool) -> GeometryOutput {

        let submodel_map: HashMap<String, SubModel> = mpd

            .submodels

            .iter()

            .map(|s| (s.name.to_lowercase(), s.clone()))

            .collect();



        let mut missing: Vec<String> = Vec::new();

        let effective_root = mpd.submodels.first().cloned();

        if instancing {

            let mut geometries: HashMap<String, PartGeometry> = HashMap::new();

            let mut instances: Vec<PartInstance> = Vec::new();

            if let Some(ref root) = effective_root {
                collect_instances(
                    root,
                    DMat4::IDENTITY,
                    16,
                    false,
                    0,
                    &submodel_map, self.parser, self.custom_parts,
                    &self.ldraw_index, self.verbose,
                    &mut geometries, &mut instances, &mut missing,
                );
                if let Some(cb) = &mut self.progress_callback { cb(); }
            }

            GeometryOutput { mode: ExportMode::Instanced { geometries, instances }, missing_parts: missing }

        } else {

            let mut accumulators: HashMap<u32, MeshAccumulator> = HashMap::new();

            if let Some(ref root) = effective_root {

                expand_merged(

                    root, DMat4::IDENTITY, 16, false, 0,

                    &submodel_map, self.parser, self.custom_parts,

                    &self.ldraw_index, self.verbose, &mut accumulators, &mut missing,

                );

                if let Some(cb) = &mut self.progress_callback { cb(); }

            }

            let meshes: Vec<MergedMesh> = accumulators

                .into_iter()

                .map(|(color_id, acc)| MergedMesh {

                    color_id,

                    vertices: acc.vertices,

                    triangles: acc.triangles,

                })

                .collect();

            GeometryOutput { mode: ExportMode::Merged(meshes), missing_parts: missing }

        }

    }

}



#[derive(Default)]

struct MeshAccumulator {

    vertex_map: HashMap<[i32; 3], u32>,

    vertices: Vec<[f32; 3]>,

    triangles: Vec<[u32; 3]>,

}



impl MeshAccumulator {

    fn add_vertex(&mut self, v: [f32; 3]) -> u32 {
        const Q: f32 = 16384.0;
        let key = [(v[0] * Q).round() as i32, (v[1] * Q).round() as i32, (v[2] * Q).round() as i32];

        if let Some(&idx) = self.vertex_map.get(&key) {
            return idx;
        }

        let idx = self.vertices.len() as u32;
        self.vertex_map.insert(key, idx);
        self.vertices.push(v);
        idx
    }



    fn add_triangle(&mut self, v0: [f32; 3], v1: [f32; 3], v2: [f32; 3]) {

        let i0 = self.add_vertex(v0);

        let i1 = self.add_vertex(v1);

        let i2 = self.add_vertex(v2);

        self.triangles.push([i0, i1, i2]);

    }

}



fn transform_vertex(transform: DMat4, v: DVec3) -> [f32; 3] {

    let world = transform.transform_point3(v);

    let x = (world.x * LDRAW_SCALE) as f32;

    let y = (world.z * LDRAW_SCALE) as f32;

    let z = (-world.y * LDRAW_SCALE) as f32;

    [x, y, z]

}



fn dmat4_to_blender_mat4(m: DMat4) -> Mat4 {

    // 正确公式：M_fbx = A * M_ldraw * A^-1

    // A: LDraw(x,y,z) → FBX(x,z,-y)，A^-1 列为 (1,0,0),(0,0,1),(0,-1,0)

    // 结论：

    //   fbx_col0 = A * ldraw_col0

    //   fbx_col1 = A * ldraw_col2   (注意是 z_axis)

    //   fbx_col2 = -(A * ldraw_col1) (注意是 -y_axis)

    //   fbx_col3 = A * ldraw_translation * SCALE

    let a = |c: glam::DVec3| -> glam::Vec3 {

        glam::Vec3::new(c.x as f32, c.z as f32, -c.y as f32)

    };

    let s = LDRAW_SCALE as f32;

    let c0 = a(m.x_axis.truncate());

    let c1 = a(m.z_axis.truncate());

    let c2 = -a(m.y_axis.truncate());

    let t  = a(m.w_axis.truncate()) * s;

    Mat4::from_cols(c0.extend(0.0), c1.extend(0.0), c2.extend(0.0), t.extend(1.0))

}



fn resolve_file_content(

    name: &str,

    custom_parts: &HashMap<String, String>,

    ldraw_index: &HashMap<String, PathBuf>,

) -> Option<String> {

    let lower = name.to_lowercase();

    if let Some(content) = custom_parts.get(&lower) {

        return Some(content.clone());

    }

    // 索引 key 统一用 '/'，查找时也须统一

    let normalized = lower.replace('\\', "/");

    ldraw_index

        .get(&normalized)

        .or_else(|| ldraw_index.get(&lower))

        .and_then(|path| std::fs::read_to_string(path).ok())

}



#[allow(clippy::too_many_arguments)]

fn expand_merged(

    submodel: &SubModel,

    parent_transform: DMat4,

    parent_color: u32,

    parent_invert: bool,

    depth: u32,

    submodel_map: &HashMap<String, SubModel>,

    parser: &mut LDrawParser,

    custom_parts: &HashMap<String, String>,

    ldraw_index: &HashMap<String, PathBuf>,

    verbose: bool,

    accumulators: &mut HashMap<u32, MeshAccumulator>,

    missing: &mut Vec<String>,

) {

    if depth > 16 {

        eprintln!("[WARN] 递归深度超过 16 层，停止展开");

        return;

    }

    let mut invert_next = false;

    for line in &submodel.lines {

        match line {

            LDrawLine::InvertNext => { invert_next = true; }

            LDrawLine::Type1(ref1) => {

                let current_invert = invert_next;

                invert_next = false;

                let child_color = if ref1.color == 16 || ref1.color == 24 { parent_color } else { ref1.color };

                let child_transform = parent_transform * ref1.transform;

                let ref_reflects = ref1.transform.determinant() < 0.0;

                let should_invert =

                    (submodel.bfc_mode == BfcMode::CertifyCw) ^ current_invert ^ parent_invert ^ ref_reflects;

                let file_lower = ref1.file.to_lowercase();

                if let Some(child_submodel) = submodel_map.get(&file_lower) {

                    let child_clone = child_submodel.clone();

                    expand_merged(&child_clone, child_transform, child_color, should_invert,

                        depth + 1, submodel_map, parser, custom_parts, ldraw_index, verbose, accumulators, missing);

                } else {

                    match resolve_file_content(&ref1.file, custom_parts, ldraw_index) {

                        Some(content) => match parser.parse_file(&ref1.file, &content) {

                            Ok(parsed) => {

                                let parsed_clone = (*parsed).clone();

                                if let Some(first) = parsed_clone.submodels.into_iter().next() {

                                    expand_merged(&first, child_transform, child_color, should_invert,

                                        depth + 1, submodel_map, parser, custom_parts, ldraw_index, verbose, accumulators, missing);

                                }

                            }

                            Err(e) => eprintln!("[WARN] 解析文件 {} 失败: {}", ref1.file, e),

                        },

                        None => {
                            missing.push(ref1.file.clone());
                            if verbose { eprintln!("[WARN] 找不到零件文件: {}（已跳过）", ref1.file); }
                        }

                    }

                }

            }

            LDrawLine::Type3(tri) => {

                let current_invert = invert_next;

                invert_next = false;

                let color = if tri.color == 16 || tri.color == 24 { parent_color } else { tri.color };

                let should_invert =

                    (submodel.bfc_mode == BfcMode::CertifyCw) ^ current_invert ^ parent_invert;

                let mut verts = tri.vertices.map(|v| transform_vertex(parent_transform, v));

                if should_invert { verts.swap(0, 2); }

                let acc = accumulators.entry(color).or_insert_with(MeshAccumulator::default);

                acc.add_triangle(verts[0], verts[1], verts[2]);

            }

            LDrawLine::Type4(quad) => {

                let current_invert = invert_next;

                invert_next = false;

                let color = if quad.color == 16 || quad.color == 24 { parent_color } else { quad.color };

                let should_invert =

                    (submodel.bfc_mode == BfcMode::CertifyCw) ^ current_invert ^ parent_invert;

                let mut verts = quad.vertices.map(|v| transform_vertex(parent_transform, v));

                if should_invert { verts.swap(1, 3); }

                let acc = accumulators.entry(color).or_insert_with(MeshAccumulator::default);

                acc.add_triangle(verts[0], verts[1], verts[2]);

                acc.add_triangle(verts[0], verts[2], verts[3]);

            }

            LDrawLine::Other => {}

        }

    }

}



fn is_leaf_part(file: &str) -> bool {

    file.to_lowercase().ends_with(".dat")

}



#[allow(clippy::too_many_arguments)]

fn collect_part_geometry(

    submodel: &SubModel,

    local_transform: DMat4,

    parent_invert: bool,

    depth: u32,

    submodel_map: &HashMap<String, SubModel>,

    parser: &mut LDrawParser,

    custom_parts: &HashMap<String, String>,

    ldraw_index: &HashMap<String, PathBuf>,

    verbose: bool,

    acc: &mut MeshAccumulator,

    missing: &mut Vec<String>,

) {

    if depth > 16 { return; }

    let mut invert_next = false;

    for line in &submodel.lines {

        match line {

            LDrawLine::InvertNext => { invert_next = true; }

            LDrawLine::Type1(ref1) => {

                let current_invert = invert_next;

                invert_next = false;

                let child_transform = local_transform * ref1.transform;

                let ref_reflects = ref1.transform.determinant() < 0.0;

                let should_invert =

                    (submodel.bfc_mode == BfcMode::CertifyCw) ^ current_invert ^ parent_invert ^ ref_reflects;

                let file_lower = ref1.file.to_lowercase();

                if let Some(child_submodel) = submodel_map.get(&file_lower) {

                    let child_clone = child_submodel.clone();

                    collect_part_geometry(&child_clone, child_transform, should_invert, depth + 1,

                        submodel_map, parser, custom_parts, ldraw_index, verbose, acc, missing);

                } else {

                    match resolve_file_content(&ref1.file, custom_parts, ldraw_index) {

                        Some(content) => match parser.parse_file(&ref1.file, &content) {

                            Ok(parsed) => {

                                let parsed_clone = (*parsed).clone();

                                if let Some(first) = parsed_clone.submodels.into_iter().next() {

                                    collect_part_geometry(&first, child_transform, should_invert,

                                        depth + 1, submodel_map, parser, custom_parts, ldraw_index, verbose, acc, missing);

                                }

                            }

                            Err(e) => eprintln!("[WARN] 解析文件 {} 失败: {}", ref1.file, e),

                        },

                        None => {
                            missing.push(ref1.file.clone());
                            if verbose { eprintln!("[WARN] 找不到零件文件: {}（已跳过）", ref1.file); }
                        }

                    }

                }

            }

            LDrawLine::Type3(tri) => {

                let current_invert = invert_next;

                invert_next = false;

                let should_invert =

                    (submodel.bfc_mode == BfcMode::CertifyCw) ^ current_invert ^ parent_invert;

                let mut verts = tri.vertices.map(|v| transform_vertex(local_transform, v));

                if should_invert { verts.swap(0, 2); }

                acc.add_triangle(verts[0], verts[1], verts[2]);

            }

            LDrawLine::Type4(quad) => {

                let current_invert = invert_next;

                invert_next = false;

                let should_invert =

                    (submodel.bfc_mode == BfcMode::CertifyCw) ^ current_invert ^ parent_invert;

                let mut verts = quad.vertices.map(|v| transform_vertex(local_transform, v));

                if should_invert { verts.swap(1, 3); }

                acc.add_triangle(verts[0], verts[1], verts[2]);

                acc.add_triangle(verts[0], verts[2], verts[3]);

            }

            LDrawLine::Other => {}

        }

    }

}



#[allow(clippy::too_many_arguments)]

fn collect_instances(

    submodel: &SubModel,

    parent_transform: DMat4,

    parent_color: u32,

    parent_invert: bool,

    depth: u32,

    submodel_map: &HashMap<String, SubModel>,

    parser: &mut LDrawParser,

    custom_parts: &HashMap<String, String>,

    ldraw_index: &HashMap<String, PathBuf>,

    verbose: bool,

    geometries: &mut HashMap<String, PartGeometry>,

    instances: &mut Vec<PartInstance>,

    missing: &mut Vec<String>,

) {

    if depth > 16 {

        eprintln!("[WARN] 递归深度超过 16 层，停止展开");

        return;

    }

    let mut invert_next = false;

    for line in &submodel.lines {

        match line {

            LDrawLine::InvertNext => { invert_next = true; }

            LDrawLine::Type1(ref1) => {

                let current_invert = invert_next;

                invert_next = false;

                let child_color = if ref1.color == 16 || ref1.color == 24 { parent_color } else { ref1.color };

                let child_transform = parent_transform * ref1.transform;

                let ref_reflects = ref1.transform.determinant() < 0.0;

                let should_invert =

                    (submodel.bfc_mode == BfcMode::CertifyCw) ^ current_invert ^ parent_invert ^ ref_reflects;

                let file_lower = ref1.file.to_lowercase();

                if is_leaf_part(&ref1.file) {

                    // 正/反朝向分别缓存，避免同一零件两种用法混用

                    let geom_key = if should_invert {

                        format!("{}_INV", file_lower)

                    } else {

                        file_lower.clone()

                    };

                    if !geometries.contains_key(&geom_key) {

                        let mut acc = MeshAccumulator::default();

                        if let Some(child_submodel) = submodel_map.get(&file_lower) {

                            let child_clone = child_submodel.clone();

                            collect_part_geometry(&child_clone, DMat4::IDENTITY, should_invert, 0,

                                submodel_map, parser, custom_parts, ldraw_index, verbose, &mut acc, missing);

                        } else {

                            match resolve_file_content(&ref1.file, custom_parts, ldraw_index) {

                                Some(content) => match parser.parse_file(&ref1.file, &content) {

                                    Ok(parsed) => {

                                        let parsed_clone = (*parsed).clone();

                                        if let Some(first) = parsed_clone.submodels.into_iter().next() {

                                            collect_part_geometry(&first, DMat4::IDENTITY, should_invert, 0,

                                                submodel_map, parser, custom_parts, ldraw_index, verbose, &mut acc, missing);

                                        }

                                    }

                                    Err(e) => eprintln!("[WARN] 解析文件 {} 失败: {}", ref1.file, e),

                                },

                                None => {
                                    missing.push(ref1.file.clone());
                                    if verbose { eprintln!("[WARN] 找不到零件文件: {}（已跳过）", ref1.file); }
                                }

                            }

                        }

                        geometries.insert(geom_key.clone(), PartGeometry {

                            part_name: geom_key.clone(),

                            vertices: acc.vertices,

                            triangles: acc.triangles,

                        });

                    }

                    let world_mat = dmat4_to_blender_mat4(child_transform);

                    instances.push(PartInstance {

                        part_name: geom_key,

                        color_id: child_color,

                        transform: world_mat,

                    });

                } else {

                    if let Some(child_submodel) = submodel_map.get(&file_lower) {

                        let child_clone = child_submodel.clone();

                        collect_instances(&child_clone, child_transform, child_color, should_invert,

                            depth + 1, submodel_map, parser, custom_parts, ldraw_index, verbose,

                            geometries, instances, missing);

                    } else {

                        match resolve_file_content(&ref1.file, custom_parts, ldraw_index) {

                            Some(content) => match parser.parse_file(&ref1.file, &content) {

                                Ok(parsed) => {

                                    let parsed_clone = (*parsed).clone();

                                    if let Some(first) = parsed_clone.submodels.into_iter().next() {

                                        collect_instances(&first, child_transform, child_color, should_invert,

                                            depth + 1, submodel_map, parser, custom_parts, ldraw_index, verbose,

                                            geometries, instances, missing);

                                    }

                                }

                                Err(e) => eprintln!("[WARN] 解析文件 {} 失败: {}", ref1.file, e),

                            },

                            None => {
                                missing.push(ref1.file.clone());
                                if verbose { eprintln!("[WARN] 找不到零件文件: {}（已跳过）", ref1.file); }
                            }

                        }

                    }

                }

            }

            LDrawLine::Type3(_) | LDrawLine::Type4(_) => { invert_next = false; }

            LDrawLine::Other => {}

        }

    }

}

