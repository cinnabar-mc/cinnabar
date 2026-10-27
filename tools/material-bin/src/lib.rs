//! A reader for RenderDragon compiled material definitions (`*.material.bin`, format version
//! 25 as shipped with Bedrock 1.26), so their shader passes, variant flags and shader sources
//! can be inspected as behaviour evidence. Only the structure is decoded; nothing here ships
//! game data, and encrypted definitions are reported rather than read.

mod reader;

use anyhow::{Context, Result, bail, ensure};
use reader::Reader;

/// The file's leading and trailing magic number.
pub const MAGIC: u64 = 0x0a11_da1a;
/// The definition type name every compiled material starts with.
pub const DEFINITION: &str = "RenderDragon.CompiledMaterialDefinition";
/// The only format version this reader decodes.
pub const VERSION: u64 = 25;
/// The encryption tag of an unencrypted definition (`NONE` as a little-endian four-cc).
pub const UNENCRYPTED: u32 = u32::from_le_bytes(*b"ENON");

/// One decoded material definition.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub parent: Option<String>,
    pub buffers: Vec<Buffer>,
    pub uniforms: Vec<Uniform>,
    pub passes: Vec<Pass>,
}

/// A sampler, image or storage buffer the material binds.
#[derive(Clone, Debug, PartialEq)]
pub struct Buffer {
    pub name: String,
    pub register: u16,
    pub access: u8,
    pub precision: u8,
    pub unordered_access: bool,
    pub kind: u8,
    pub format: String,
    /// The texture loaded when nothing else is bound, such as `white`.
    pub default_texture: Option<String>,
    /// A structured buffer's element type and stride.
    pub element: Option<(String, u32)>,
}

/// A uniform and its default value.
#[derive(Clone, Debug, PartialEq)]
pub struct Uniform {
    pub name: String,
    /// 2 is `vec4`, 3 `mat3`, 4 `mat4`.
    pub kind: u16,
    pub count: u32,
    pub default: Option<Vec<f32>>,
}

/// A render pass of the material and its compiled variants.
#[derive(Clone, Debug, PartialEq)]
pub struct Pass {
    pub name: String,
    /// One character per platform: whether the pass is supported there.
    pub platforms: String,
    pub fallback: String,
    pub blend_mode: Option<u16>,
    /// Flag values a variant takes when it does not set the flag.
    pub default_flags: Vec<(String, String)>,
    /// An opaque word, zero in most passes and a hash in prepass and deferred passes.
    pub tag: u32,
    pub variants: Vec<Variant>,
}

/// One combination of flag values and its shaders.
#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub supported: bool,
    pub flags: Vec<(String, String)>,
    pub shaders: Vec<Shader>,
}

impl Variant {
    /// The value of `flag` in this variant, falling back to the pass's default.
    pub fn flag<'a>(&'a self, pass: &'a Pass, flag: &str) -> Option<&'a str> {
        self.flags
            .iter()
            .chain(&pass.default_flags)
            .find(|(name, _)| name == flag)
            .map(|(_, value)| value.as_str())
    }
}

/// One compiled shader stage.
#[derive(Clone, Debug, PartialEq)]
pub struct Shader {
    /// `Vertex`, `Fragment`, `Compute` or `Unknown`.
    pub stage: String,
    /// The shader language, such as `ESSL_310`.
    pub platform: String,
    pub inputs: Vec<String>,
    pub hash: u64,
    /// The bgfx shader binary.
    pub blob: Vec<u8>,
}

impl Shader {
    /// The shader's source text, for text platforms, read from the bgfx binary.
    pub fn source(&self) -> Result<&str> {
        let mut reader = Reader::new(&self.blob);
        let magic = reader.bytes(4)?;
        ensure!(
            matches!(&magic[..3], b"VSH" | b"FSH" | b"CSH"),
            "not a bgfx shader binary"
        );
        reader.u32()?;
        for _ in 0..reader.u16()? {
            let name = usize::from(reader.u8()?);
            reader.bytes(name + 6)?;
        }
        let size = reader.u32()? as usize;
        std::str::from_utf8(reader.bytes(size)?).context("shader source is not UTF-8")
    }
}

/// Decodes a compiled material definition.
pub fn decode(bytes: &[u8]) -> Result<Material> {
    let mut reader = Reader::new(bytes);
    ensure!(reader.u64()? == MAGIC, "not a compiled material");
    ensure!(reader.string()? == DEFINITION, "not a material definition");
    let version = reader.u64()?;
    ensure!(
        version == VERSION,
        "format version {version} is not {VERSION}"
    );
    let encryption = reader.u32()?;
    if encryption != UNENCRYPTED {
        bail!(
            "encrypted ({})",
            String::from_utf8_lossy(&encryption.to_be_bytes())
        );
    }
    let name = reader.string()?;
    let parent = reader.optional(Reader::string)?;
    let buffers = (0..reader.u8()?)
        .map(|_| buffer(&mut reader))
        .collect::<Result<_>>()?;
    let uniforms = (0..reader.u16()?)
        .map(|_| uniform(&mut reader))
        .collect::<Result<_>>()?;
    let overrides = reader.u16()?;
    ensure!(overrides == 0, "uniform overrides are not decoded");
    let passes = (0..reader.u16()?)
        .map(|_| pass(&mut reader))
        .collect::<Result<_>>()?;
    ensure!(reader.u64()? == MAGIC, "missing trailing magic");
    ensure!(reader.is_empty(), "trailing bytes after the definition");
    Ok(Material {
        name,
        parent,
        buffers,
        uniforms,
        passes,
    })
}

/// One buffer entry: binding, format, default texture and structured element type.
fn buffer(reader: &mut Reader<'_>) -> Result<Buffer> {
    let name = reader.string()?;
    let register = reader.u16()?;
    let access = reader.u8()?;
    let precision = reader.u8()?;
    let unordered_access = reader.u8()? != 0;
    let kind = reader.u8()?;
    let format = reader.string()?;
    reader.u32()?;
    reader.u8()?;
    ensure!(
        reader.u8()? == 0,
        "buffer {name}: sampler state is not decoded"
    );
    let default_texture = reader.optional(Reader::string)?;
    ensure!(reader.u8()? == 0, "buffer {name}: unknown field");
    let element = reader.optional(|reader| Ok((reader.string()?, reader.u32()?)))?;
    Ok(Buffer {
        name,
        register,
        access,
        precision,
        unordered_access,
        kind,
        format,
        default_texture,
        element,
    })
}

/// One uniform entry and its optional default value.
fn uniform(reader: &mut Reader<'_>) -> Result<Uniform> {
    let name = reader.string()?;
    let kind = reader.u16()?;
    let count = reader.u32()?;
    let floats = match kind {
        1 => 0,
        2 => 4,
        3 => 12,
        4 => 16,
        other => bail!("uniform {name}: unknown type {other}"),
    };
    let default = reader.optional(|reader| (0..floats).map(|_| reader.f32()).collect())?;
    Ok(Uniform {
        name,
        kind,
        count,
        default,
    })
}

/// `count` flag name and value pairs.
fn flags(reader: &mut Reader<'_>, count: u16) -> Result<Vec<(String, String)>> {
    (0..count)
        .map(|_| Ok((reader.string()?, reader.string()?)))
        .collect()
}

/// One pass: its platforms, fallback, blend mode, default flags and variants.
fn pass(reader: &mut Reader<'_>) -> Result<Pass> {
    let name = reader.string()?;
    let platforms = reader.string()?;
    let fallback = reader.string()?;
    let blend_mode = reader.optional(Reader::u16)?;
    let count = reader.u16()?;
    let default_flags = flags(reader, count)?;
    let tag = reader.u32()?;
    let variants = (0..reader.u16()?)
        .map(|_| variant(reader))
        .collect::<Result<_>>()?;
    Ok(Pass {
        name,
        platforms,
        fallback,
        blend_mode,
        default_flags,
        tag,
        variants,
    })
}

/// One variant: its flag values and shaders.
fn variant(reader: &mut Reader<'_>) -> Result<Variant> {
    let supported = reader.u8()? != 0;
    let flag_count = reader.u16()?;
    let shader_count = reader.u16()?;
    let flags = flags(reader, flag_count)?;
    let shaders = (0..shader_count)
        .map(|_| shader(reader))
        .collect::<Result<_>>()?;
    Ok(Variant {
        supported,
        flags,
        shaders,
    })
}

/// One shader: stage, platform, input names, hash and bgfx binary.
fn shader(reader: &mut Reader<'_>) -> Result<Shader> {
    let stage = reader.string()?;
    let platform = reader.string()?;
    reader.u8()?;
    reader.u8()?;
    let inputs = (0..reader.u16()?)
        .map(|_| {
            let name = reader.string()?;
            reader.bytes(4)?;
            reader.optional(Reader::u8)?;
            reader.optional(Reader::u8)?;
            Ok(name)
        })
        .collect::<Result<_>>()?;
    let hash = reader.u64()?;
    let size = reader.u32()? as usize;
    let blob = reader.bytes(size)?.to_vec();
    Ok(Shader {
        stage,
        platform,
        inputs,
        hash,
        blob,
    })
}
