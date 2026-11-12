use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use glam::{EulerRot, Mat3, Mat4, Vec3};

use crate::color_resolver::{material_name, resolve_color, MaterialType};
use crate::config::srgb_to_linear_rgb;
use crate::geometry_builder::{MergedMesh, PartGeometry, PartInstance};
use crate::types::{ExportError, ExportStats};

pub struct FbxExporter;

// ── ID 分配基址 ───────────────────────────────────────────────────────────────
const MAT_ID_BASE: i64    = 1_000_000;
const GEOM_ID_BASE: i64   = 2_000_000;
const MODEL_ID_BASE: i64  = 3_000_000;
const SCENE_MODEL_ID: i64 = 9_000_000;
const DOCUMENT_ID: i64    = 5_678_901;
const FBX_VERSION: u32    = 7400;

// ── 材质属性 ──────────────────────────────────────────────────────────────────
struct MatProps { specular: f64, shininess: f64, opacity: f64 }

fn mat_props(mt: &MaterialType, alpha: f32) -> MatProps {
    match mt {
        MaterialType::Normal      => MatProps { specular: 0.1,  shininess: 20.0,  opacity: 1.0 },
        MaterialType::Trans       => MatProps { specular: 0.1,  shininess: 20.0,  opacity: alpha as f64 },
        MaterialType::Chrome      => MatProps { specular: 1.0,  shininess: 100.0, opacity: 1.0 },
        MaterialType::Pearlescent => MatProps { specular: 0.5,  shininess: 50.0,  opacity: 1.0 },
        MaterialType::Metal       => MatProps { specular: 0.5,  shininess: 50.0,  opacity: 1.0 },
        MaterialType::Rubber      => MatProps { specular: 0.05, shininess: 5.0,   opacity: 1.0 },
    }
}

// ── Binary FBX property buffer ────────────────────────────────────────────────
struct PropBuf {
    data: Vec<u8>,
    count: u32,
}

impl PropBuf {
    fn new() -> Self { Self { data: Vec::new(), count: 0 } }

    fn i32(&mut self, v: i32) -> &mut Self {
        self.data.push(b'I');
        self.data.extend_from_slice(&v.to_le_bytes());
        self.count += 1; self
    }

    fn i64(&mut self, v: i64) -> &mut Self {
        self.data.push(b'L');
        self.data.extend_from_slice(&v.to_le_bytes());
        self.count += 1; self
    }

    fn f64(&mut self, v: f64) -> &mut Self {
        self.data.push(b'D');
        self.data.extend_from_slice(&v.to_bits().to_le_bytes());
        self.count += 1; self
    }

    fn bool_val(&mut self, v: bool) -> &mut Self {
        self.data.push(b'C');
        self.data.push(v as u8);
        self.count += 1; self
    }

    fn str_val(&mut self, s: &str) -> &mut Self {
        self.data.push(b'S');
        self.data.extend_from_slice(&(s.len() as u32).to_le_bytes());
        self.data.extend_from_slice(s.as_bytes());
        self.count += 1; self
    }

    /// Binary FBX 节点名称格式："name\x00\x01Class"
    fn fbx_name(&mut self, name: &str, class: &str) -> &mut Self {
        let mut s = Vec::with_capacity(name.len() + 2 + class.len());
        s.extend_from_slice(name.as_bytes());
        s.push(0x00);
        s.push(0x01);
        s.extend_from_slice(class.as_bytes());
        self.data.push(b'S');
        self.data.extend_from_slice(&(s.len() as u32).to_le_bytes());
        self.data.extend_from_slice(&s);
        self.count += 1; self
    }

    fn f64_arr(&mut self, arr: &[f64]) -> &mut Self {
        self.data.push(b'd');
        self.data.extend_from_slice(&(arr.len() as u32).to_le_bytes());
        self.data.extend_from_slice(&0u32.to_le_bytes());                       // encoding = raw
        self.data.extend_from_slice(&((arr.len() * 8) as u32).to_le_bytes());   // compressed_length
        for &v in arr { self.data.extend_from_slice(&v.to_bits().to_le_bytes()); }
        self.count += 1; self
    }

    fn i32_arr(&mut self, arr: &[i32]) -> &mut Self {
        self.data.push(b'i');
        self.data.extend_from_slice(&(arr.len() as u32).to_le_bytes());
        self.data.extend_from_slice(&0u32.to_le_bytes());
        self.data.extend_from_slice(&((arr.len() * 4) as u32).to_le_bytes());
        for &v in arr { self.data.extend_from_slice(&v.to_le_bytes()); }
        self.count += 1; self
    }
}

// ── Binary FBX node writer ────────────────────────────────────────────────────
struct BinBuf { data: Vec<u8> }

impl BinBuf {
    fn with_capacity(n: usize) -> Self { Self { data: Vec::with_capacity(n) } }

    fn write_magic(&mut self) {
        // 20 bytes name + \x00\x1a\x00 + version u32 LE = 27 bytes total
        self.data.extend_from_slice(b"Kaydara FBX Binary  \x00\x1a\x00");
        self.data.extend_from_slice(&FBX_VERSION.to_le_bytes());
    }

    /// Leaf node: no children, no null sentinel
    fn leaf(&mut self, name: &str, p: &PropBuf) {
        let patch = self.data.len();
        self.data.extend_from_slice(&[0u8; 4]);                                   // end_offset
        self.data.extend_from_slice(&p.count.to_le_bytes());
        self.data.extend_from_slice(&(p.data.len() as u32).to_le_bytes());
        self.data.push(name.len() as u8);
        self.data.extend_from_slice(name.as_bytes());
        self.data.extend_from_slice(&p.data);
        let end = self.data.len() as u32;
        self.data[patch..patch + 4].copy_from_slice(&end.to_le_bytes());
    }

    /// Begin a parent node; returns the patch position for end_offset
    fn begin(&mut self, name: &str, p: &PropBuf) -> usize {
        let patch = self.data.len();
        self.data.extend_from_slice(&[0u8; 4]);
        self.data.extend_from_slice(&p.count.to_le_bytes());
        self.data.extend_from_slice(&(p.data.len() as u32).to_le_bytes());
        self.data.push(name.len() as u8);
        self.data.extend_from_slice(name.as_bytes());
        self.data.extend_from_slice(&p.data);
        patch
    }

    /// End a parent node: write 13-byte null sentinel, patch end_offset
    fn end(&mut self, patch: usize) {
        self.data.extend_from_slice(&[0u8; 13]);
        let end = self.data.len() as u32;
        self.data[patch..patch + 4].copy_from_slice(&end.to_le_bytes());
    }

    fn write_footer(&mut self) {
        self.data.extend_from_slice(&[0u8; 13]);                        // null sentinel for root list
        let pad = (16 - (self.data.len() % 16)) % 16;
        for _ in 0..pad { self.data.push(0); }
        self.data.extend_from_slice(&[0u8; 16]);
        self.data.extend_from_slice(&FBX_VERSION.to_le_bytes());
        self.data.extend_from_slice(&[0u8; 120]);
        self.data.extend_from_slice(&[0u8; 16]);
    }
}

// ── Section helpers ───────────────────────────────────────────────────────────

fn section_header(w: &mut BinBuf) {
    let h = w.begin("FBXHeaderExtension", &PropBuf::new());

    let mut p = PropBuf::new(); p.i32(1003);
    w.leaf("FBXHeaderVersion", &p);

    let mut p = PropBuf::new(); p.i32(FBX_VERSION as i32);
    w.leaf("FBXVersion", &p);

    let mut p = PropBuf::new(); p.i32(0);
    w.leaf("EncryptionType", &p);

    let ts = w.begin("CreationTimeStamp", &PropBuf::new());
    let mut p = PropBuf::new(); p.i32(1000); w.leaf("Version", &p);
    let mut p = PropBuf::new(); p.i32(2025); w.leaf("Year", &p);
    let mut p = PropBuf::new(); p.i32(1);    w.leaf("Month", &p);
    let mut p = PropBuf::new(); p.i32(1);    w.leaf("Day", &p);
    let mut p = PropBuf::new(); p.i32(0);    w.leaf("Hour", &p);
    let mut p = PropBuf::new(); p.i32(0);    w.leaf("Minute", &p);
    let mut p = PropBuf::new(); p.i32(0);    w.leaf("Second", &p);
    let mut p = PropBuf::new(); p.i32(0);    w.leaf("Millisecond", &p);
    w.end(ts);

    let mut p = PropBuf::new(); p.str_val("io-to-fbx-converter");
    w.leaf("Creator", &p);

    w.end(h);
}

fn section_global_settings(w: &mut BinBuf) {
    let gs = w.begin("GlobalSettings", &PropBuf::new());

    let mut p = PropBuf::new(); p.i32(1000);
    w.leaf("Version", &p);

    let p70 = w.begin("Properties70", &PropBuf::new());

    macro_rules! pi {
        ($n:expr, $v:expr) => {{
            let mut p = PropBuf::new();
            p.str_val($n).str_val("int").str_val("Integer").str_val("").i32($v);
            w.leaf("P", &p);
        }};
    }
    macro_rules! pd {
        ($n:expr, $v:expr) => {{
            let mut p = PropBuf::new();
            p.str_val($n).str_val("double").str_val("Number").str_val("").f64($v);
            w.leaf("P", &p);
        }};
    }

    pi!("UpAxis",                  2);   // Z up（与 transform_vertex 输出一致）
    pi!("UpAxisSign",              1);
    pi!("FrontAxis",               1);   // Y front（Blender 原生惯例）
    pi!("FrontAxisSign",          -1);   // -Y 是正面
    pi!("CoordAxis",               0);
    pi!("CoordAxisSign",           1);
    pi!("OriginalUpAxis",         -1);
    pi!("OriginalUpAxisSign",      1);
    pd!("UnitScaleFactor",         1.0);
    pd!("OriginalUnitScaleFactor", 100.0);

    w.end(p70);
    w.end(gs);
}

fn section_documents(w: &mut BinBuf) {
    let docs = w.begin("Documents", &PropBuf::new());

    let mut p = PropBuf::new(); p.i32(1);
    w.leaf("Count", &p);

    let mut p = PropBuf::new(); p.i64(DOCUMENT_ID).str_val("").str_val("Scene");
    let doc = w.begin("Document", &p);

    let p70 = w.begin("Properties70", &PropBuf::new());
    w.end(p70);

    let mut p = PropBuf::new(); p.i64(0);
    w.leaf("RootNode", &p);

    w.end(doc);
    w.end(docs);
}

fn section_references(w: &mut BinBuf) {
    let r = w.begin("References", &PropBuf::new());
    w.end(r);
}

fn section_definitions(w: &mut BinBuf, model_count: i32, geom_count: i32, mat_count: i32) {
    let defs = w.begin("Definitions", &PropBuf::new());

    let mut p = PropBuf::new(); p.i32(100);
    w.leaf("Version", &p);

    let mut p = PropBuf::new(); p.i32(1 + model_count + geom_count + mat_count);
    w.leaf("Count", &p);

    macro_rules! obj_type {
        ($name:expr, $count:expr) => {{
            let mut p = PropBuf::new(); p.str_val($name);
            let ot = w.begin("ObjectType", &p);
            let mut p = PropBuf::new(); p.i32($count);
            w.leaf("Count", &p);
            w.end(ot);
        }};
    }

    obj_type!("GlobalSettings", 1);
    obj_type!("Model",    model_count);
    obj_type!("Geometry", geom_count);
    obj_type!("Material", mat_count);

    w.end(defs);
}

fn section_takes(w: &mut BinBuf) {
    let takes = w.begin("Takes", &PropBuf::new());
    let mut p = PropBuf::new(); p.str_val("");
    w.leaf("Current", &p);
    w.end(takes);
}

// ── Object node writers ───────────────────────────────────────────────────────

fn node_scene_model(w: &mut BinBuf) {
    let mut p = PropBuf::new();
    p.i64(SCENE_MODEL_ID).fbx_name("Scene", "Model").str_val("Null");
    let m = w.begin("Model", &p);

    let mut p = PropBuf::new(); p.i32(232); w.leaf("Version", &p);

    let p70 = w.begin("Properties70", &PropBuf::new());
    write_trs_props(w, 0.0, 0.0, 0.0,  0.0, 0.0, 0.0,  1.0, 1.0, 1.0);
    w.end(p70);

    let mut p = PropBuf::new(); p.bool_val(true);  w.leaf("Shading", &p);
    let mut p = PropBuf::new(); p.str_val("CullingOff"); w.leaf("Culling", &p);
    w.end(m);
}

fn write_trs_props(
    w: &mut BinBuf,
    tx: f64, ty: f64, tz: f64,
    rx: f64, ry: f64, rz: f64,
    sx: f64, sy: f64, sz: f64,
) {
    let mut p = PropBuf::new();
    p.str_val("Lcl Translation").str_val("Lcl Translation").str_val("").str_val("A")
     .f64(tx).f64(ty).f64(tz);
    w.leaf("P", &p);

    let mut p = PropBuf::new();
    p.str_val("Lcl Rotation").str_val("Lcl Rotation").str_val("").str_val("A")
     .f64(rx).f64(ry).f64(rz);
    w.leaf("P", &p);

    let mut p = PropBuf::new();
    p.str_val("Lcl Scaling").str_val("Lcl Scaling").str_val("").str_val("A")
     .f64(sx).f64(sy).f64(sz);
    w.leaf("P", &p);
}

fn node_model_identity(w: &mut BinBuf, model_id: i64, name: &str) {
    let mut p = PropBuf::new(); p.i64(model_id).fbx_name(name, "Model").str_val("Mesh");
    let m = w.begin("Model", &p);

    let mut p = PropBuf::new(); p.i32(232); w.leaf("Version", &p);

    let p70 = w.begin("Properties70", &PropBuf::new());
    write_trs_props(w, 0.0, 0.0, 0.0,  0.0, 0.0, 0.0,  1.0, 1.0, 1.0);
    w.end(p70);

    let mut p = PropBuf::new(); p.bool_val(true);  w.leaf("Shading", &p);
    let mut p = PropBuf::new(); p.str_val("CullingOff"); w.leaf("Culling", &p);
    w.end(m);
}

fn node_model_trs(w: &mut BinBuf, model_id: i64, name: &str, transform: &Mat4) {
    let t = transform.w_axis.truncate();
    let sx = transform.x_axis.truncate().length();
    let sy = transform.y_axis.truncate().length();
    let sz = transform.z_axis.truncate().length();
    let rx_col = if sx > 1e-10 { transform.x_axis.truncate() / sx } else { Vec3::X };
    let ry_col = if sy > 1e-10 { transform.y_axis.truncate() / sy } else { Vec3::Y };
    let rz_col = if sz > 1e-10 { transform.z_axis.truncate() / sz } else { Vec3::Z };
    let (ez, ey, ex) = Mat3::from_cols(rx_col, ry_col, rz_col).to_euler(EulerRot::ZYX);

    let mut p = PropBuf::new(); p.i64(model_id).fbx_name(name, "Model").str_val("Mesh");
    let m = w.begin("Model", &p);

    let mut p = PropBuf::new(); p.i32(232); w.leaf("Version", &p);

    let p70 = w.begin("Properties70", &PropBuf::new());
    write_trs_props(
        w,
        t.x as f64, t.y as f64, t.z as f64,
        ex.to_degrees() as f64, ey.to_degrees() as f64, ez.to_degrees() as f64,
        sx as f64, sy as f64, sz as f64,
    );
    w.end(p70);

    let mut p = PropBuf::new(); p.bool_val(true);  w.leaf("Shading", &p);
    let mut p = PropBuf::new(); p.str_val("CullingOff"); w.leaf("Culling", &p);
    w.end(m);
}

fn node_material(w: &mut BinBuf, mat_id: i64, color_id: u32, color_correction: bool) {
    let color  = resolve_color(color_id);
    let name   = material_name(&color);
    let props  = mat_props(&color.material_type, color.alpha);
    let [r, g, b] = if color_correction { srgb_to_linear_rgb(color.rgb) } else { color.rgb };

    let mut p = PropBuf::new(); p.i64(mat_id).fbx_name(&name, "Material").str_val("");
    let mat = w.begin("Material", &p);

    let mut p = PropBuf::new(); p.i32(102);       w.leaf("Version", &p);
    let mut p = PropBuf::new(); p.str_val("Phong"); w.leaf("ShadingModel", &p);
    let mut p = PropBuf::new(); p.i32(0);           w.leaf("MultiLayer", &p);

    let p70 = w.begin("Properties70", &PropBuf::new());

    macro_rules! color_prop {
        ($n:expr, $r:expr, $g:expr, $b:expr) => {{
            let mut p = PropBuf::new();
            p.str_val($n).str_val("Color").str_val("").str_val("A")
             .f64($r as f64).f64($g as f64).f64($b as f64);
            w.leaf("P", &p);
        }};
    }
    macro_rules! num_prop {
        ($n:expr, $v:expr) => {{
            let mut p = PropBuf::new();
            p.str_val($n).str_val("double").str_val("Number").str_val("").f64($v);
            w.leaf("P", &p);
        }};
    }

    color_prop!("DiffuseColor",  r, g, b);
    color_prop!("SpecularColor", r, g, b);
    num_prop!("SpecularFactor",     props.specular);
    num_prop!("Shininess",          props.shininess);
    num_prop!("Opacity",            props.opacity);
    if props.opacity < 1.0 {
        num_prop!("TransparencyFactor", 1.0 - props.opacity);
    }

    w.end(p70);
    w.end(mat);
}

fn node_geometry(w: &mut BinBuf, geom_id: i64, name: &str, vertices: &[[f32; 3]], triangles: &[[u32; 3]]) {
    // Build flat arrays
    let verts_flat: Vec<f64> = vertices.iter()
        .flat_map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
        .collect();

    let poly_idx: Vec<i32> = triangles.iter()
        .flat_map(|tri| [tri[0] as i32, tri[1] as i32, !(tri[2] as i32)])
        .collect();

    let mut p = PropBuf::new(); p.i64(geom_id).fbx_name(name, "Geometry").str_val("Mesh");
    let geom = w.begin("Geometry", &p);

    let mut p = PropBuf::new(); p.f64_arr(&verts_flat);
    w.leaf("Vertices", &p);

    let mut p = PropBuf::new(); p.i32_arr(&poly_idx);
    w.leaf("PolygonVertexIndex", &p);

    let mut p = PropBuf::new(); p.i32(124);
    w.leaf("GeometryVersion", &p);

    // LayerElementMaterial
    {
        let mut p = PropBuf::new(); p.i32(0);
        let lem = w.begin("LayerElementMaterial", &p);

        let mut p = PropBuf::new(); p.i32(101);              w.leaf("Version", &p);
        let mut p = PropBuf::new(); p.str_val("");            w.leaf("Name", &p);
        let mut p = PropBuf::new(); p.str_val("AllSame");     w.leaf("MappingInformationType", &p);
        let mut p = PropBuf::new(); p.str_val("IndexToDirect"); w.leaf("ReferenceInformationType", &p);
        let mut p = PropBuf::new(); p.i32_arr(&[0i32]);       w.leaf("Materials", &p);

        w.end(lem);
    }

    // Layer
    {
        let mut p = PropBuf::new(); p.i32(0);
        let layer = w.begin("Layer", &p);

        let mut p = PropBuf::new(); p.i32(100); w.leaf("Version", &p);

        let le_m = w.begin("LayerElement", &PropBuf::new());
        let mut p = PropBuf::new(); p.str_val("LayerElementMaterial"); w.leaf("Type", &p);
        let mut p = PropBuf::new(); p.i32(0); w.leaf("TypedIndex", &p);
        w.end(le_m);

        w.end(layer);
    }

    w.end(geom);
}

// ── Connection helper ─────────────────────────────────────────────────────────
fn conn_oo(w: &mut BinBuf, src: i64, dst: i64) {
    let mut p = PropBuf::new(); p.str_val("OO").i64(src).i64(dst);
    w.leaf("C", &p);
}

// ── count_material_type ───────────────────────────────────────────────────────
fn count_material_type(mt: &MaterialType) -> (usize, usize, usize, usize) {
    match mt {
        MaterialType::Normal | MaterialType::Rubber => (1, 0, 0, 0),
        MaterialType::Trans                          => (0, 1, 0, 0),
        MaterialType::Chrome                         => (0, 0, 1, 0),
        MaterialType::Pearlescent | MaterialType::Metal => (0, 0, 0, 1),
    }
}

// ── Public API ────────────────────────────────────────────────────────────────
impl FbxExporter {

    /// 合并模式导出：每种颜色一个 Geometry + Model + Material
    pub fn export_merged(
        output: &Path,
        meshes: &[MergedMesh],
        color_correction: bool,
    ) -> Result<ExportStats, ExportError> {
        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let mut sorted_meshes: Vec<&MergedMesh> = meshes.iter().collect();
        sorted_meshes.sort_by_key(|m| m.color_id);
        let n = sorted_meshes.len();

        let mut w = BinBuf::with_capacity(32 * 1024 * 1024);
        w.write_magic();

        section_header(&mut w);
        section_global_settings(&mut w);
        section_documents(&mut w);
        section_references(&mut w);
        section_definitions(&mut w, (n + 1) as i32, n as i32, n as i32);

        let objects = w.begin("Objects", &PropBuf::new());

        node_scene_model(&mut w);

        for (i, mesh) in sorted_meshes.iter().enumerate() {
            node_material(&mut w, MAT_ID_BASE + i as i64, mesh.color_id, color_correction);
        }
        for (i, mesh) in sorted_meshes.iter().enumerate() {
            let name = format!("Mesh_Color_{}", mesh.color_id);
            node_geometry(&mut w, GEOM_ID_BASE + i as i64, &name, &mesh.vertices, &mesh.triangles);
        }
        for (i, mesh) in sorted_meshes.iter().enumerate() {
            let name = format!("Mesh_Color_{}", mesh.color_id);
            node_model_identity(&mut w, MODEL_ID_BASE + i as i64, &name);
            let _ = mesh;
        }

        w.end(objects);

        let conns = w.begin("Connections", &PropBuf::new());
        conn_oo(&mut w, SCENE_MODEL_ID, 0);
        for i in 0..n {
            let mat_id   = MAT_ID_BASE   + i as i64;
            let geom_id  = GEOM_ID_BASE  + i as i64;
            let model_id = MODEL_ID_BASE + i as i64;
            conn_oo(&mut w, model_id, SCENE_MODEL_ID);
            conn_oo(&mut w, geom_id,  model_id);
            conn_oo(&mut w, mat_id,   model_id);
        }
        w.end(conns);

        section_takes(&mut w);
        w.write_footer();

        fs::write(output, &w.data)?;

        let mut stats = ExportStats {
            mesh_count: n, vertex_count: 0, triangle_count: 0, material_count: n,
            normal_material_count: 0, trans_material_count: 0,
            chrome_material_count: 0, metal_pearl_material_count: 0,
        };
        for mesh in &sorted_meshes {
            stats.vertex_count   += mesh.vertices.len();
            stats.triangle_count += mesh.triangles.len();
            let color = resolve_color(mesh.color_id);
            let (a, b, c, d) = count_material_type(&color.material_type);
            stats.normal_material_count      += a;
            stats.trans_material_count       += b;
            stats.chrome_material_count      += c;
            stats.metal_pearl_material_count += d;
        }
        Ok(stats)
    }

    /// 实例化模式导出：每个唯一零件一个 Geometry，每个实例一个 Model
    pub fn export_instanced(
        output: &Path,
        geometries: &HashMap<String, PartGeometry>,
        instances: &[PartInstance],
        color_correction: bool,
    ) -> Result<ExportStats, ExportError> {
        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let mut sorted_geoms: Vec<(&String, &PartGeometry)> = geometries.iter().collect();
        sorted_geoms.sort_by_key(|(k, _)| k.as_str());

        let geom_id_map: HashMap<&str, i64> = sorted_geoms.iter()
            .enumerate()
            .map(|(i, (k, _))| (k.as_str(), GEOM_ID_BASE + i as i64))
            .collect();

        // 按 (RGB, 材质类型, alpha) 去重材质球，相同外观的颜色ID共用同一个材质
        fn color_dedup_key(color_id: u32) -> (u32, u8, u8) {
            let c = resolve_color(color_id);
            let [r, g, b] = c.rgb;
            let rgb_u32 = ((r * 255.0) as u32) << 16
                        | ((g * 255.0) as u32) << 8
                        | (b * 255.0) as u32;
            let alpha_u8 = (c.alpha * 255.0) as u8;
            let mat_key = match c.material_type {
                MaterialType::Normal      => 0u8,
                MaterialType::Trans       => 1,
                MaterialType::Chrome      => 2,
                MaterialType::Pearlescent => 3,
                MaterialType::Metal       => 4,
                MaterialType::Rubber      => 5,
            };
            (rgb_u32, mat_key, alpha_u8)
        }

        // 收集所有用到的 color_id，按去重 key 分组，每组只生成一个材质球
        let mut used_color_ids: Vec<u32> = instances.iter()
            .map(|i| i.color_id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        used_color_ids.sort();

        // dedup_key -> (mat_id, 代表color_id)
        let mut key_to_mat: HashMap<(u32, u8, u8), (i64, u32)> = HashMap::new();
        let mut mat_counter: i64 = 0;
        for &cid in &used_color_ids {
            let key = color_dedup_key(cid);
            key_to_mat.entry(key).or_insert_with(|| {
                let id = MAT_ID_BASE + mat_counter;
                mat_counter += 1;
                (id, cid)
            });
        }
        // color_id -> mat_id
        let mat_id_map: HashMap<u32, i64> = used_color_ids.iter()
            .map(|&cid| (cid, key_to_mat[&color_dedup_key(cid)].0))
            .collect();
        // 去重后实际要写入的材质列表（代表color_id排序）
        let mut unique_mats: Vec<(i64, u32)> = key_to_mat.values().copied().collect();
        unique_mats.sort_by_key(|(id, _)| *id);

        let mut w = BinBuf::with_capacity(64 * 1024 * 1024);
        w.write_magic();

        section_header(&mut w);
        section_global_settings(&mut w);
        section_documents(&mut w);
        section_references(&mut w);
        section_definitions(
            &mut w,
            (instances.len() + 1) as i32,
            sorted_geoms.len() as i32,
            unique_mats.len() as i32,
        );

        let objects = w.begin("Objects", &PropBuf::new());

        node_scene_model(&mut w);

        for &(mat_id, color_id) in &unique_mats {
            node_material(&mut w, mat_id, color_id, color_correction);
        }
        for (_, geom) in &sorted_geoms {
            let geom_id = geom_id_map[geom.part_name.as_str()];
            node_geometry(&mut w, geom_id, &geom.part_name, &geom.vertices, &geom.triangles);
        }
        for (inst_idx, inst) in instances.iter().enumerate() {
            let model_id = MODEL_ID_BASE + inst_idx as i64;
            let name = format!("{}_inst_{}", inst.part_name, inst_idx);
            node_model_trs(&mut w, model_id, &name, &inst.transform);
        }

        w.end(objects);

        let conns = w.begin("Connections", &PropBuf::new());
        conn_oo(&mut w, SCENE_MODEL_ID, 0);
        for (inst_idx, inst) in instances.iter().enumerate() {
            let model_id = MODEL_ID_BASE + inst_idx as i64;
            let geom_id  = geom_id_map[inst.part_name.as_str()];
            let mat_id   = mat_id_map[&inst.color_id];
            conn_oo(&mut w, model_id, SCENE_MODEL_ID);
            conn_oo(&mut w, geom_id,  model_id);
            conn_oo(&mut w, mat_id,   model_id);
        }
        w.end(conns);

        section_takes(&mut w);
        w.write_footer();

        fs::write(output, &w.data)?;

        let mut stats = ExportStats {
            mesh_count: instances.len(), vertex_count: 0, triangle_count: 0,
            material_count: unique_mats.len(),
            normal_material_count: 0, trans_material_count: 0,
            chrome_material_count: 0, metal_pearl_material_count: 0,
        };
        for (_, geom) in &sorted_geoms {
            stats.vertex_count   += geom.vertices.len();
            stats.triangle_count += geom.triangles.len();
        }
        for &color_id in &used_color_ids {
            let color = resolve_color(color_id);
            let (a, b, c, d) = count_material_type(&color.material_type);
            stats.normal_material_count      += a;
            stats.trans_material_count       += b;
            stats.chrome_material_count      += c;
            stats.metal_pearl_material_count += d;
        }
        Ok(stats)
    }
}
