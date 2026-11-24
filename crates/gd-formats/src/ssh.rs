//! `.ssh` compiled shader effects ("SSH\x04"). One file holds one effect
//! compiled for several hardware profiles; each profile is a list of chunks:
//! a value table (shader objects + literal state values), parameter
//! declarations, and techniques ("styles") made of passes of render states.
//!
//! Loader: `GraphicsShader2::Initialize` (Engine.dll) picks
//! the profile named by the device (D3D11 renderer → "DirectX11"); chunk
//! readers `GraphicsShader2::ReadValues` (values), `GraphicsShader2::ReadParams` /
//! `GraphicsShader2::ReadParamBlock` (params), `GraphicsShader2::ReadTechniques` /
//! `GraphicsShader2::ReadPass` (techniques); state application `GraphicsShader2::ApplyStates`.
//! DirectX11 bytecode is DXBC (vs_5_0/ps_5_0) with RDEF reflection intact;
//! the GeForce*/Radeon*/R200 profiles carry D3D9 SM1–3 bytecode.
use crate::util::{Reader, latin1};
use anyhow::{Context, Result, bail, ensure};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    Bool(bool),
    Str(String),
    Float(f32),
    Float2([f32; 2]),
    /// Type 9 (12 = colour3 alias).
    Float3([f32; 3]),
    /// Type 11 (13 = colour4 alias).
    Float4([f32; 4]),
    /// Type 15 (7 alias): a texture resource name.
    Texture(String),
    VertexShader(ShaderObject),
    PixelShader(ShaderObject),
}

/// Constant binding of one parameter into a shader's constant storage.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstBinding {
    /// DX11: constant buffer slot (cb0..cb3). D3D9: -1 (float registers).
    pub slot: i32,
    /// DX11: byte offset in the cbuffer. D3D9: first float4 register.
    pub offset: i32,
    /// DX11: bytes. D3D9: register count.
    pub size: i32,
    pub flag: bool,
    /// Index into `Profile::params`.
    pub param: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShaderObject {
    pub bindings: Vec<ConstBinding>,
    /// DXBC container (DirectX11/XBoxOne) or D3D9 token stream.
    pub bytecode: Vec<u8>,
}

/// Parameter types (file codes; 12→9 and 13→11 are folded by the engine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamType {
    Int,
    Bool,
    Str,
    Float4x3,
    Float4x4,
    VertexShader,
    PixelShader,
    Float2,
    Float3,
    Float,
    Float4,
    Texture,
    Other(u32),
}

impl ShaderObject {
    /// DXBC container? (A few stock DirectX11 profiles still hold D3D9 ps_1_1 tokens.)
    pub fn is_dxbc(&self) -> bool {
        self.bytecode.starts_with(b"DXBC")
    }
}

impl ParamType {
    fn from_code(c: u32) -> Self {
        match c {
            0 => Self::Int,
            1 => Self::Bool,
            2 => Self::Str,
            3 => Self::Float4x3,
            4 => Self::Float4x4,
            5 => Self::VertexShader,
            6 => Self::PixelShader,
            7 | 15 => Self::Texture,
            8 => Self::Float2,
            9 | 12 => Self::Float3,
            10 => Self::Float,
            11 | 13 => Self::Float4,
            c => Self::Other(c),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub type_code: u32,
    pub ty: ParamType,
    pub count: u32,
    /// First skipped u32: 1 on per-material params (textures, colours).
    pub material: bool,
    /// Second skipped u32 (1 on params that have a default value).
    pub unknown: u32,
    /// Default value: index into `Profile::values`.
    pub default: Option<u32>,
}

/// One render-state assignment inside a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateAssign {
    pub state: u32,
    /// Sampler/texture stage index for sampler & texture states.
    pub index: u32,
    /// true: `value` indexes `Profile::values`; false: indexes `Profile::params`.
    pub literal: bool,
    pub value: u32,
}

#[derive(Debug, Clone)]
pub struct Pass {
    pub states: Vec<StateAssign>,
}

/// A technique ("style", e.g. `ambientLightStyle`, `dLightDiffuseStyle`).
#[derive(Debug, Clone)]
pub struct Technique {
    pub name: String,
    pub sort_order: i32,
    pub passes: Vec<Pass>,
}

#[derive(Debug, Clone, Default)]
pub struct Profile {
    pub name: String,
    pub values: Vec<Value>,
    pub params: Vec<Param>,
    pub techniques: Vec<Technique>,
    /// Chunk ids other than 0/1/2 (skipped by the engine).
    pub unknown_chunks: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct Ssh {
    pub profiles: Vec<Profile>,
}

fn string(r: &mut Reader) -> Result<String> {
    let n = r.u32()? as usize;
    ensure!(n <= 0x100000, "string too long");
    Ok(latin1(r.bytes(n)?))
}

fn shader_object(r: &mut Reader) -> Result<ShaderObject> {
    let n = r.u32()? as usize;
    let mut bindings = Vec::with_capacity(r.capacity(n, 20));
    for _ in 0..n {
        bindings.push(ConstBinding {
            slot: r.i32()?,
            offset: r.i32()?,
            size: r.i32()?,
            flag: r.u32()? != 0,
            param: r.i32()?,
        });
    }
    let len = r.u32()? as usize;
    Ok(ShaderObject {
        bindings,
        bytecode: r.bytes(len)?.to_vec(),
    })
}

fn values(r: &mut Reader) -> Result<Vec<Value>> {
    let n = r.u32()?;
    ensure!(n <= 1000, "too many values ({n})");
    let mut out = Vec::new();
    for _ in 0..n {
        let t = r.u32()?;
        out.push(match t {
            0 => Value::Int(r.i32()?),
            1 => Value::Bool(r.u32()? != 0),
            2 => Value::Str(string(r)?),
            5 => Value::VertexShader(shader_object(r)?),
            6 => Value::PixelShader(shader_object(r)?),
            7 | 15 => Value::Texture(string(r)?),
            8 => Value::Float2([r.f32()?, r.f32()?]),
            9 | 12 => Value::Float3([r.f32()?, r.f32()?, r.f32()?]),
            10 => Value::Float(r.f32()?),
            11 | 13 => Value::Float4([r.f32()?, r.f32()?, r.f32()?, r.f32()?]),
            t => bail!("unknown value type {t} at {:#x}", r.pos),
        });
    }
    Ok(out)
}

fn params(r: &mut Reader) -> Result<Vec<Param>> {
    let n = r.u32()?;
    ensure!(n <= 1000, "too many params ({n})");
    let mut out = Vec::new();
    for _ in 0..n {
        let name = string(r)?;
        let type_code = r.u32()?;
        let count = r.u32()?;
        let material = r.u32()? != 0;
        let unknown = r.u32()?;
        let has_default = r.u32()? != 0;
        let d = r.u32()?;
        out.push(Param {
            name,
            type_code,
            ty: ParamType::from_code(type_code),
            count,
            material,
            unknown,
            default: has_default.then_some(d),
        });
    }
    Ok(out)
}

fn techniques(r: &mut Reader) -> Result<Vec<Technique>> {
    let n = r.u32()?;
    ensure!(n <= 1000, "too many techniques ({n})");
    let mut out = Vec::new();
    for _ in 0..n {
        let name = string(r)?;
        let sort_order = r.i32()?;
        let np = r.u32()?;
        let mut passes = Vec::new();
        for _ in 0..np {
            let ns = r.u32()?;
            let mut states = Vec::new();
            for _ in 0..ns {
                states.push(StateAssign {
                    state: r.u32()?,
                    index: r.u32()?,
                    literal: r.u32()? != 0,
                    value: r.u32()?,
                });
            }
            passes.push(Pass { states });
        }
        out.push(Technique {
            name,
            sort_order,
            passes,
        });
    }
    Ok(out)
}

impl Ssh {
    pub fn parse(b: &[u8]) -> Result<Self> {
        ensure!(
            b.len() >= 4 && &b[..3] == b"SSH" && b[3] == 4,
            "not an SSH v4 file"
        );
        let mut r = Reader::at(b, 4);
        let mut profiles = Vec::new();
        while r.remaining() > 0 {
            let name = string(&mut r)?;
            let size = r.u32()? as usize;
            let body = r.bytes(size).with_context(|| format!("profile {name}"))?;
            let mut p = Profile {
                name: name.clone(),
                ..Default::default()
            };
            let mut c = Reader::new(body);
            while c.remaining() > 0 {
                let id = c.u32()?;
                let len = c.u32()? as usize;
                let data = c.bytes(len)?;
                let mut d = Reader::new(data);
                match id {
                    0 => p.params = params(&mut d)?,
                    1 => p.values = values(&mut d)?,
                    2 => p.techniques = techniques(&mut d)?,
                    other => p.unknown_chunks.push(other),
                }
                if id <= 2 {
                    ensure!(
                        d.remaining() == 0,
                        "{name}: chunk {id} has {} trailing bytes",
                        d.remaining()
                    );
                }
            }
            profiles.push(p);
        }
        Ok(Self { profiles })
    }

    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }
}

impl Profile {
    /// Resolve a state's value: literal → `values[v]`; otherwise the param's default.
    pub fn state_value(&self, s: &StateAssign) -> Option<&Value> {
        if s.literal {
            self.values.get(s.value as usize)
        } else {
            let p = self.params.get(s.value as usize)?;
            self.values.get(p.default? as usize)
        }
    }
    fn int_of(&self, s: &StateAssign) -> Option<i32> {
        match self.state_value(s)? {
            Value::Int(i) => Some(*i),
            Value::Bool(b) => Some(*b as i32),
            Value::Float(f) => Some(*f as i32),
            _ => None,
        }
    }
    /// Human-readable description of one state assignment.
    pub fn describe(&self, s: &StateAssign) -> String {
        let pname = |i: u32| {
            self.params
                .get(i as usize)
                .map(|p| p.name.as_str())
                .unwrap_or("?")
        };
        let v = self.int_of(s);
        let e = |f: fn(i32) -> &'static str| v.map(f).unwrap_or("?").to_string();
        let (name, val) = match s.state {
            0 => (
                "Texture",
                format!(
                    "t{} = {}",
                    s.index,
                    if s.literal {
                        format!("{:?}", self.state_value(s))
                    } else {
                        pname(s.value).into()
                    }
                ),
            ),
            1 => ("VertexShader", pname(s.value).into()),
            2 => ("PixelShader", pname(s.value).into()),
            3 => ("SrcBlend", e(blend_factor)),
            4 => ("DstBlend", e(blend_factor)),
            5 => ("DepthFunc", e(compare_func)),
            6 => ("AlphaTestFunc(D3D9 only)", e(compare_func)),
            7 => ("AlphaRef(D3D9 only)", format!("{v:?}")),
            8 => ("ColorWrite", format!("{}", v.unwrap_or(-1) != 0)),
            9 => ("DepthWrite", format!("{}", v.unwrap_or(-1) != 0)),
            10 => ("AddressU", format!("s{} {}", s.index, e(address_mode))),
            11 => ("AddressV", format!("s{} {}", s.index, e(address_mode))),
            15 => ("MinFilter", format!("s{} {}", s.index, e(filter))),
            16 => ("MagFilter", format!("s{} {}", s.index, e(filter))),
            17 => ("MipFilter", format!("s{} {}", s.index, e(filter))),
            18 => ("CullMode", e(cull_mode)),
            19 => ("AlphaWrite", format!("{}", v.unwrap_or(-1) != 0)),
            20 => ("DepthBias", format!("{:?}", self.state_value(s))),
            21 => ("DepthSlopeBias", format!("{:?}", self.state_value(s))),
            22 => ("BlendFactor", format!("{:?}", self.state_value(s))),
            23 => ("BlendOp", e(blend_op)),
            24 => ("StencilFunc", e(compare_func)),
            25 => ("StencilRef", format!("{:?}", self.state_value(s))),
            26 => ("StencilMask", format!("{:?}", self.state_value(s))),
            27 => ("StencilPassOp", e(stencil_op)),
            28 => ("StencilFailOp", e(stencil_op)),
            29 => (
                "SamplerState6(ignored by D3D11)",
                format!("s{} {v:?}", s.index),
            ),
            30 => ("AlphaToCoverage", format!("{}", v.unwrap_or(-1) != 0)),
            _ => ("Unknown", format!("{s:?}")),
        };
        format!("{name}: {val}")
    }
}

/// Engine `RenderBlendFactor` → name (Direct3D11.dll `ConvertRenderBlendFactor`).
pub fn blend_factor(v: i32) -> &'static str {
    match v {
        0 => "One",
        2 => "DstColor",
        3 => "SrcColor",
        5 => "DstAlpha",
        6 => "SrcAlpha",
        7 => "OneMinusDstAlpha",
        8 => "OneMinusSrcAlpha",
        9 => "OneMinusDstColor",
        10 => "OneMinusSrcColor",
        11 => "BlendFactor",
        12 => "OneMinusBlendFactor",
        _ => "Zero",
    }
}
/// Depth/stencil test function (`SetDepthTestFunction`). 7+ disables the test.
pub fn compare_func(v: i32) -> &'static str {
    match v {
        0 => "Never",
        1 => "Equal",
        2 => "Less",
        3 => "LessEqual",
        4 => "Greater",
        5 => "GreaterEqual",
        6 => "NotEqual",
        _ => "Always",
    }
}
/// `SetCullingMode` (D3D11 default winding: clockwise = front).
pub fn cull_mode(v: i32) -> &'static str {
    match v {
        1 => "Front",
        2 => "Back",
        _ => "None",
    }
}
/// Sampler address mode (`SetSamplerState`).
pub fn address_mode(v: i32) -> &'static str {
    match v {
        1 => "Clamp",
        2 => "Border",
        _ => "Wrap",
    }
}
/// Sampler filter: 4 = point, anything else linear.
pub fn filter(v: i32) -> &'static str {
    if v == 4 { "Point" } else { "Linear" }
}
pub fn blend_op(v: i32) -> &'static str {
    match v {
        1 => "Subtract",
        2 => "ReverseSubtract",
        3 => "Min",
        4 => "Max",
        _ => "Add",
    }
}
/// `SetStencilWriteMode`.
pub fn stencil_op(v: i32) -> &'static str {
    match v {
        1 => "Replace",
        2 => "IncrementWrap",
        3 => "DecrementWrap",
        4 => "Zero",
        5 => "Invert",
        6 => "IncrementClamp",
        7 => "DecrementClamp",
        _ => "Keep",
    }
}

/// Minimal DXBC reflection (RDEF + ISGN/OSGN) for the DirectX11 profile.
#[derive(Debug, Clone, Default)]
pub struct Dxbc {
    /// e.g. "vs_5_0".
    pub target: String,
    pub resources: Vec<BoundResource>,
    pub cbuffers: Vec<CBuffer>,
    pub inputs: Vec<SigElement>,
    pub outputs: Vec<SigElement>,
    pub chunks: Vec<[u8; 4]>,
}

#[derive(Debug, Clone)]
pub struct BoundResource {
    pub name: String,
    /// 0 cbuffer, 1 tbuffer, 2 texture, 3 sampler, ...
    pub kind: u32,
    pub dimension: u32,
    pub bind_point: u32,
    pub bind_count: u32,
}

#[derive(Debug, Clone)]
pub struct CBuffer {
    pub name: String,
    pub size: u32,
    pub vars: Vec<CBVar>,
}

#[derive(Debug, Clone)]
pub struct CBVar {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    /// D3D_SHADER_VARIABLE_CLASS (0 scalar, 1 vector, 2/3 matrix) and dims.
    pub class: u16,
    pub rows: u16,
    pub cols: u16,
    /// D3D_SVF_USED.
    pub used: bool,
}

#[derive(Debug, Clone)]
pub struct SigElement {
    pub semantic: String,
    pub index: u32,
    pub system_value: u32,
    pub register: u32,
    pub mask: u8,
}

fn cstr(b: &[u8], off: usize) -> String {
    let s = b.get(off..).unwrap_or(&[]);
    latin1(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())])
}

impl Dxbc {
    pub fn parse(b: &[u8]) -> Result<Self> {
        ensure!(b.len() >= 32 && &b[..4] == b"DXBC", "not DXBC");
        let mut r = Reader::at(b, 28);
        let n = r.u32()? as usize;
        let mut out = Dxbc::default();
        for _ in 0..n {
            let off = r.u32()? as usize;
            let mut c = Reader::at(b, off);
            let fourcc: [u8; 4] = c.bytes(4)?.try_into().unwrap();
            let len = c.u32()? as usize;
            let data = c.bytes(len)?;
            out.chunks.push(fourcc);
            match &fourcc {
                b"RDEF" => out.rdef(data)?,
                b"ISGN" => out.inputs = signature(data)?,
                b"OSGN" => out.outputs = signature(data)?,
                _ => {}
            }
        }
        Ok(out)
    }

    fn rdef(&mut self, d: &[u8]) -> Result<()> {
        let mut r = Reader::new(d);
        let (cb_count, cb_off, bind_count, bind_off) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
        let minor = r.u8()?;
        let major = r.u8()?;
        let prog = r.u16()?;
        self.target = format!(
            "{}_{major}_{minor}",
            match prog {
                0xFFFE => "vs",
                0xFFFF => "ps",
                0xFFFD => "gs",
                0xFFFC => "cs",
                _ => "xs",
            }
        );
        let var_size = if major >= 5 { 40 } else { 24 };
        for i in 0..bind_count as usize {
            let mut b = Reader::at(d, bind_off as usize + 32 * i);
            let name = cstr(d, b.u32()? as usize);
            let kind = b.u32()?;
            let _ret = b.u32()?;
            let dimension = b.u32()?;
            let _samples = b.u32()?;
            let bind_point = b.u32()?;
            let bind_count = b.u32()?;
            self.resources.push(BoundResource {
                name,
                kind,
                dimension,
                bind_point,
                bind_count,
            });
        }
        for i in 0..cb_count as usize {
            let mut c = Reader::at(d, cb_off as usize + 24 * i);
            let name = cstr(d, c.u32()? as usize);
            let nvars = c.u32()? as usize;
            let var_off = c.u32()? as usize;
            let size = c.u32()?;
            let mut vars = Vec::new();
            for v in 0..nvars {
                let mut vr = Reader::at(d, var_off + var_size * v);
                let name = cstr(d, vr.u32()? as usize);
                let offset = vr.u32()?;
                let size = vr.u32()?;
                let flags = vr.u32()?;
                let mut t = Reader::at(d, vr.u32()? as usize);
                let class = t.u16()?;
                let _ty = t.u16()?;
                let rows = t.u16()?;
                let cols = t.u16()?;
                vars.push(CBVar {
                    name,
                    offset,
                    size,
                    class,
                    rows,
                    cols,
                    used: flags & 2 != 0,
                });
            }
            self.cbuffers.push(CBuffer { name, size, vars });
        }
        Ok(())
    }
}

fn signature(d: &[u8]) -> Result<Vec<SigElement>> {
    let mut r = Reader::new(d);
    let n = r.u32()? as usize;
    r.skip(4)?;
    let mut out = Vec::new();
    for _ in 0..n {
        let name = cstr(d, r.u32()? as usize);
        let index = r.u32()?;
        let system_value = r.u32()?;
        let _comp = r.u32()?;
        let register = r.u32()?;
        let mask = r.u8()?;
        r.skip(3)?;
        out.push(SigElement {
            semantic: name,
            index,
            system_value,
            register,
            mask,
        });
    }
    Ok(out)
}
