//! Mesh/animation tools: validate all .msh/.anm, print info, export glTF.
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use gd_formats::anm::{self, Animation};
use gd_formats::msh::{Coords, Mesh, ParamValue};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse every .msh/.anm under the given directories and report stats
    Check { dirs: Vec<PathBuf> },
    /// Print a summary of one .msh or .anm
    Info { file: PathBuf },
    /// Export a mesh (optionally skinned + one animation) to .glb
    Glb {
        msh: PathBuf,
        out: PathBuf,
        #[arg(long)]
        anm: Option<PathBuf>,
        /// Bake the animation pose at this frame into the vertices instead of
        /// exporting a skin
        #[arg(long)]
        bake_frame: Option<f32>,
    },
}

fn ext(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn check(dirs: &[PathBuf]) -> Result<()> {
    let files: Vec<PathBuf> = dirs.iter().flat_map(|d| gd_tools::walk_files(d)).collect();
    let mut stats: BTreeMap<String, usize> = BTreeMap::new();
    let mut errors = 0usize;
    let (mut nm, mut na) = (0, 0);
    let mut bump = |k: String| *stats.entry(k).or_default() += 1;
    for f in &files {
        let e = ext(f);
        if e != "msh" && e != "anm" {
            continue;
        }
        let data = std::fs::read(f)?;
        if e == "msh" {
            nm += 1;
            match Mesh::parse(&data) {
                Ok(m) => {
                    bump(format!("msh version {}", m.version));
                    let decl: Vec<u32> = m.vertex_attrs.iter().map(|a| a.id()).collect();
                    bump(format!(
                        "msh vertex decl {:?} stride {}",
                        decl, m.vertex_stride
                    ));
                    let mut ids: Vec<&str> = Vec::new();
                    if m.physics.is_some() {
                        ids.push("physics")
                    }
                    if !m.contours.is_empty() {
                        ids.push("contours")
                    }
                    if !m.portals.is_empty() {
                        ids.push("portals")
                    }
                    if !m.blended_faces.is_empty() {
                        ids.push("blended")
                    }
                    if !m.paintable_faces.is_empty() {
                        ids.push("paintable")
                    }
                    if !m.unknown_chunks.is_empty() {
                        ids.push("UNKNOWN")
                    }
                    for i in ids {
                        bump(format!("msh has {i}"));
                    }
                    for mat in &m.materials {
                        bump(format!("shader {}", mat.shader.to_ascii_lowercase()));
                        for p in &mat.params {
                            let t = match p.value {
                                ParamValue::Int(_) => "int",
                                ParamValue::Bool(_) => "bool",
                                ParamValue::String(_) => "string",
                                ParamValue::Float4x3(_) => "float4x3",
                                ParamValue::Float4x4(_) => "float4x4",
                                ParamValue::Texture(_) => "texture",
                                ParamValue::Float2(_) => "float2",
                                ParamValue::Float3(_) => "float3",
                                ParamValue::Float(_) => "float",
                                ParamValue::Float4(_) => "float4",
                            };
                            bump(format!("param type {t}"));
                        }
                    }
                    for b in &m.mif {
                        bump(format!("mif block {}", b.kind));
                    }
                }
                Err(err) => {
                    errors += 1;
                    eprintln!("ERR {}: {err:#}", f.display());
                }
            }
        } else {
            na += 1;
            match Animation::parse(&data) {
                Ok(a) => {
                    bump(format!("anm version {}", a.version));
                    bump(format!("anm fps {}", a.frame_rate));
                    for b in &a.trailer {
                        bump(format!("anm block {}", b.kind));
                    }
                }
                Err(err) => {
                    errors += 1;
                    eprintln!("ERR {}: {err:#}", f.display());
                }
            }
        }
    }
    for (k, v) in &stats {
        println!("{v:8}  {k}");
    }
    println!("{nm} meshes, {na} animations, {errors} errors");
    if errors > 0 {
        anyhow::bail!("{errors} files failed");
    }
    Ok(())
}

fn info(file: &Path) -> Result<()> {
    let data = std::fs::read(file)?;
    if ext(file) == "anm" {
        let a = Animation::parse(&data)?;
        println!(
            "ANM v{} frames {} fps {} bones {}",
            a.version,
            a.num_frames,
            a.frame_rate,
            a.tracks.len()
        );
        for t in &a.tracks {
            let k = t.keys.first().copied().unwrap_or(anm::IDENTITY_KEY);
            println!(
                "  {:24} t{:?} r{:?} s{:?}",
                t.bone, k.translation, k.rotation, k.scale
            );
        }
        for c in a.callback_points() {
            println!("  callback {} @ {}", c.name, c.frame);
        }
        for e in a.create_entities() {
            println!("  create {} @ {} on {}", e.entity, e.frame, e.attach);
        }
        return Ok(());
    }
    let m = Mesh::parse(&data)?;
    println!("MSH v{} mif {:?}", m.version, m.mif_path);
    println!(
        "verts {} stride {} attrs {:?}",
        m.num_vertices, m.vertex_stride, m.vertex_attrs
    );
    println!(
        "tris {} bbox {:?} sections {:?}",
        m.indices.len() / 3,
        m.bounding_box,
        m.sections
    );
    for (i, fs) in m.face_sets.iter().enumerate() {
        println!(
            "  set {i}: mat {} faces {}+{} palette {}",
            fs.material,
            fs.start_face,
            fs.num_faces,
            fs.bones.len()
        );
    }
    for (i, mat) in m.materials.iter().enumerate() {
        println!("  material {i}: {}", mat.shader);
        for p in &mat.params {
            println!("      {} = {:?}", p.name, p.value);
        }
    }
    for (i, b) in m.bones.iter().enumerate() {
        println!(
            "  bone {i:3} {:28} parent {:?} abs.origin {:?}",
            b.name, b.parent, b.abs.origin
        );
    }
    for h in &m.hit_boxes {
        println!(
            "  hitbox {} bone {} ext {:?} at {:?}",
            h.name, h.bone, h.extents, h.coords.origin
        );
    }
    for a in m.attach_points() {
        println!(
            "  attach {} parent {:?} at {:?}",
            a.name, a.parent, a.coords.origin
        );
    }
    Ok(())
}

// glTF export. Game object space is Y-up and used as-is for glTF; only the
// triangle winding is reversed (game faces are clockwise from the front).

struct Gltf {
    bin: Vec<u8>,
    views: Vec<String>,
    accessors: Vec<String>,
}

impl Gltf {
    fn push(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while self.bin.len() % 4 != 0 {
            self.bin.push(0);
        }
        let off = self.bin.len();
        self.bin.extend_from_slice(bytes);
        let t = target
            .map(|t| format!(",\"target\":{t}"))
            .unwrap_or_default();
        self.views.push(format!(
            "{{\"buffer\":0,\"byteOffset\":{off},\"byteLength\":{}{t}}}",
            bytes.len()
        ));
        self.views.len() - 1
    }
    fn acc_f32(
        &mut self,
        data: &[f32],
        comps: usize,
        ty: &str,
        minmax: bool,
        target: Option<u32>,
    ) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        let v = self.push(&bytes, target);
        let count = data.len() / comps;
        let mm = if minmax && count > 0 {
            let mut mn = vec![f32::MAX; comps];
            let mut mx = vec![f32::MIN; comps];
            for c in data.chunks(comps) {
                for i in 0..comps {
                    mn[i] = mn[i].min(c[i]);
                    mx[i] = mx[i].max(c[i]);
                }
            }
            format!(",\"min\":{mn:?},\"max\":{mx:?}")
        } else {
            String::new()
        };
        self.accessors.push(format!(
            "{{\"bufferView\":{v},\"componentType\":5126,\"count\":{count},\"type\":\"{ty}\"{mm}}}"
        ));
        self.accessors.len() - 1
    }
    fn acc_u16(&mut self, data: &[u16], comps: usize, ty: &str, target: Option<u32>) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        let v = self.push(&bytes, target);
        self.accessors.push(format!(
            "{{\"bufferView\":{v},\"componentType\":5123,\"count\":{},\"type\":\"{ty}\"}}",
            data.len() / comps
        ));
        self.accessors.len() - 1
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "/").replace('"', "'")
}

/// Decompose an affine Coords into glTF TRS (assumes no shear).
fn trs(c: &Coords) -> ([f32; 3], [f32; 4], [f32; 3]) {
    let len = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let mut s = [len(c.x), len(c.y), len(c.z)];
    let det = {
        let [a, b, cc] = [c.x, c.y, c.z];
        a[0] * (b[1] * cc[2] - b[2] * cc[1]) - a[1] * (b[0] * cc[2] - b[2] * cc[0])
            + a[2] * (b[0] * cc[1] - b[1] * cc[0])
    };
    if det < 0.0 {
        s[0] = -s[0];
    }
    let n = |v: [f32; 3], l: f32| if l.abs() > 1e-12 { v.map(|x| x / l) } else { v };
    let (x, y, z) = (n(c.x, s[0]), n(c.y, s[1]), n(c.z, s[2]));
    // Rotation matrix columns x,y,z -> quaternion.
    let (m00, m11, m22) = (x[0], y[1], z[2]);
    let tr = m00 + m11 + m22;
    let q = if tr > 0.0 {
        let w = (tr + 1.0).sqrt() * 2.0;
        [
            (y[2] - z[1]) / w,
            (z[0] - x[2]) / w,
            (x[1] - y[0]) / w,
            0.25 * w,
        ]
    } else if m00 > m11 && m00 > m22 {
        let w = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [
            0.25 * w,
            (y[0] + x[1]) / w,
            (z[0] + x[2]) / w,
            (y[2] - z[1]) / w,
        ]
    } else if m11 > m22 {
        let w = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [
            (y[0] + x[1]) / w,
            0.25 * w,
            (z[1] + y[2]) / w,
            (z[0] - x[2]) / w,
        ]
    } else {
        let w = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [
            (z[0] + x[2]) / w,
            (z[1] + y[2]) / w,
            0.25 * w,
            (x[1] - y[0]) / w,
        ]
    };
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    (c.origin, q.map(|v| v / l), s)
}

fn glb(msh: &Path, out: &Path, anm: Option<&Path>, bake: Option<f32>) -> Result<()> {
    let m = std::sync::Arc::new(
        Mesh::parse(&std::fs::read(msh)?).with_context(|| msh.display().to_string())?,
    );
    let anim = anm
        .map(|p| Animation::parse(&std::fs::read(p)?))
        .transpose()?;
    let v = &m.vertices;
    let skinned = !v.bone_weights.is_empty() && !m.bones.is_empty();
    let gbones = m.global_bone_indices();
    let mut positions: Vec<[f32; 3]> = v.positions.clone();
    let mut normals: Vec<[f32; 3]> = v.normals.clone();
    if let (Some(f), Some(a), true) = (bake, anim.as_ref(), skinned) {
        let (_, skin) = anm::pose_mesh(&m, a, f, false);
        for i in 0..positions.len() {
            let (mut p, mut n) = ([0.0; 3], [0.0; 3]);
            for k in 0..4 {
                let w = v.bone_weights[i][k];
                if w == 0.0 {
                    continue;
                }
                let s = &skin[gbones[i][k] as usize];
                let tp = s.transform_point(v.positions[i]);
                let tn = s.transform_vector(v.normals.get(i).copied().unwrap_or([0.0; 3]));
                for c in 0..3 {
                    p[c] += w * tp[c];
                    n[c] += w * tn[c];
                }
            }
            positions[i] = p;
            normals[i] = n;
        }
    }
    let export_skin = skinned && bake.is_none();
    let mut g = Gltf {
        bin: Vec::new(),
        views: Vec::new(),
        accessors: Vec::new(),
    };
    let flat = |a: &[[f32; 3]]| a.iter().flatten().copied().collect::<Vec<f32>>();
    let pos_acc = g.acc_f32(&flat(&positions), 3, "VEC3", true, Some(34962));
    let mut attrs = format!("\"POSITION\":{pos_acc}");
    if !normals.is_empty() {
        let nrm: Vec<[f32; 3]> = normals
            .iter()
            .map(|n| {
                let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
                n.map(|c| c / l)
            })
            .collect();
        let a = g.acc_f32(&flat(&nrm), 3, "VEC3", false, Some(34962));
        attrs += &format!(",\"NORMAL\":{a}");
    }
    for (i, uv) in v.uvs.iter().enumerate() {
        let d: Vec<f32> = uv.iter().flatten().copied().collect();
        let a = g.acc_f32(&d, 2, "VEC2", false, Some(34962));
        attrs += &format!(",\"TEXCOORD_{i}\":{a}");
    }
    if !v.colors.is_empty() {
        // Stored B,G,R,A -> RGBA floats.
        let d: Vec<f32> = v
            .colors
            .iter()
            .flat_map(|c| [c[2], c[1], c[0], c[3]].map(|b| b as f32 / 255.0))
            .collect();
        let a = g.acc_f32(&d, 4, "VEC4", false, Some(34962));
        attrs += &format!(",\"COLOR_0\":{a}");
    }
    if export_skin {
        let j: Vec<u16> = gbones.iter().flatten().copied().collect();
        let ja = g.acc_u16(&j, 4, "VEC4", Some(34962));
        let w: Vec<f32> = v.bone_weights.iter().flatten().copied().collect();
        let wa = g.acc_f32(&w, 4, "VEC4", false, Some(34962));
        attrs += &format!(",\"JOINTS_0\":{ja},\"WEIGHTS_0\":{wa}");
    }
    let mut prims = Vec::new();
    let mut materials = Vec::new();
    for (i, mat) in m.materials.iter().enumerate() {
        let tex = mat.texture("baseTexture").unwrap_or("");
        materials.push(format!(
            "{{\"name\":\"{i}: {} {}\",\"pbrMetallicRoughness\":{{\"metallicFactor\":0,\"roughnessFactor\":0.8}},\"doubleSided\":false}}",
            esc(&mat.shader), esc(tex)
        ));
    }
    for fs in &m.face_sets {
        let s = fs.start_face as usize * 3;
        let mut idx: Vec<u16> = m.indices[s..s + fs.num_faces as usize * 3].to_vec();
        for t in idx.chunks_mut(3) {
            t.swap(1, 2); // clockwise -> counter-clockwise front faces
        }
        let a = g.acc_u16(&idx, 1, "SCALAR", Some(34963));
        let mat = if (fs.material as usize) < materials.len() {
            format!(",\"material\":{}", fs.material)
        } else {
            String::new()
        };
        prims.push(format!(
            "{{\"attributes\":{{{attrs}}},\"indices\":{a}{mat}}}"
        ));
    }
    let mut nodes = vec![format!(
        "{{\"name\":\"{}\",\"mesh\":0{}}}",
        esc(&msh.file_stem().unwrap_or_default().to_string_lossy()),
        if export_skin { ",\"skin\":0" } else { "" }
    )];
    let mut skins = String::new();
    let mut animations = String::new();
    let mut scene_nodes = vec![0usize];
    if export_skin {
        let base = nodes.len();
        let rest: Vec<Coords> = m
            .bones
            .iter()
            .map(|b| {
                anim.as_ref()
                    .and_then(|a| a.track(&b.name))
                    .and_then(|t| t.keys.first())
                    .map_or(b.local, |k| b.local.mul(&k.to_coords()))
            })
            .collect();
        for (i, b) in m.bones.iter().enumerate() {
            let (t, r, s) = trs(&rest[i]);
            let kids: Vec<usize> = (0..m.bones.len())
                .filter(|&c| m.bones[c].parent == Some(i))
                .map(|c| c + base)
                .collect();
            let ch = if kids.is_empty() {
                String::new()
            } else {
                format!(",\"children\":{kids:?}")
            };
            nodes.push(format!(
                "{{\"name\":\"{}\",\"translation\":{t:?},\"rotation\":{r:?},\"scale\":{s:?}{ch}}}",
                esc(&b.name)
            ));
            if b.parent.is_none() {
                scene_nodes.push(base + i);
            }
        }
        let ibm: Vec<f32> = m
            .bones
            .iter()
            .flat_map(|b| b.abs.inverse().to_mat4())
            .collect();
        let ia = g.acc_f32(&ibm, 16, "MAT4", false, None);
        let joints: Vec<usize> = (0..m.bones.len()).map(|i| i + base).collect();
        skins = format!(",\"skins\":[{{\"inverseBindMatrices\":{ia},\"joints\":{joints:?}}}]");
        if let Some(a) = &anim {
            let nf = a.num_frames.max(1) as usize;
            let fps = a.frame_rate.max(1) as f32;
            let times: Vec<f32> = (0..nf).map(|f| f as f32 / fps).collect();
            let ta = g.acc_f32(&times, 1, "SCALAR", true, None);
            let (mut samplers, mut channels) = (Vec::new(), Vec::new());
            for (i, b) in m.bones.iter().enumerate() {
                let Some(track) = a.track(&b.name) else {
                    continue;
                };
                let (mut tt, mut rr, mut ss) = (Vec::new(), Vec::new(), Vec::new());
                for f in 0..nf {
                    let k = track.keys.get(f).copied().unwrap_or(anm::IDENTITY_KEY);
                    let (t, r, s) = trs(&b.local.mul(&k.to_coords()));
                    tt.extend(t);
                    rr.extend(r);
                    ss.extend(s);
                }
                for (path, data, ty) in [
                    ("translation", tt, "VEC3"),
                    ("rotation", rr, "VEC4"),
                    ("scale", ss, "VEC3"),
                ] {
                    let comps = if ty == "VEC4" { 4 } else { 3 };
                    let oa = g.acc_f32(&data, comps, ty, false, None);
                    samplers.push(format!(
                        "{{\"input\":{ta},\"output\":{oa},\"interpolation\":\"LINEAR\"}}"
                    ));
                    channels.push(format!(
                        "{{\"sampler\":{},\"target\":{{\"node\":{},\"path\":\"{path}\"}}}}",
                        samplers.len() - 1,
                        base + i
                    ));
                }
            }
            animations = format!(
                ",\"animations\":[{{\"name\":\"{}\",\"samplers\":[{}],\"channels\":[{}]}}]",
                esc(&anm
                    .unwrap()
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()),
                samplers.join(","),
                channels.join(",")
            );
        }
    }
    while g.bin.len() % 4 != 0 {
        g.bin.push(0);
    }
    let mats = if materials.is_empty() {
        String::new()
    } else {
        format!(",\"materials\":[{}]", materials.join(","))
    };
    let json = format!(
        "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"gd-mesh\"}},\"scene\":0,\"scenes\":[{{\"nodes\":{scene_nodes:?}}}],\
\"nodes\":[{}],\"meshes\":[{{\"primitives\":[{}]}}]{mats}{skins}{animations},\
\"buffers\":[{{\"byteLength\":{}}}],\"bufferViews\":[{}],\"accessors\":[{}]}}",
        nodes.join(","),
        prims.join(","),
        g.bin.len(),
        g.views.join(","),
        g.accessors.join(",")
    );
    let mut js = json.into_bytes();
    while js.len() % 4 != 0 {
        js.push(b' ');
    }
    let total = 12 + 8 + js.len() + 8 + g.bin.len();
    let mut o = Vec::with_capacity(total);
    o.extend(b"glTF");
    o.extend(2u32.to_le_bytes());
    o.extend((total as u32).to_le_bytes());
    o.extend((js.len() as u32).to_le_bytes());
    o.extend(b"JSON");
    o.extend(&js);
    o.extend((g.bin.len() as u32).to_le_bytes());
    o.extend(b"BIN\0");
    o.extend(&g.bin);
    std::fs::write(out, o)?;
    println!(
        "wrote {} ({} verts, {} tris, skin: {export_skin})",
        out.display(),
        positions.len(),
        m.indices.len() / 3
    );
    Ok(())
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Check { dirs } => check(&dirs),
        Cmd::Info { file } => info(&file),
        Cmd::Glb {
            msh,
            out,
            anm,
            bake_frame,
        } => glb(&msh, &out, anm.as_deref(), bake_frame),
    }
}
