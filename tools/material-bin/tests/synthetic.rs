//! Decodes a synthetic definition written field by field, so the format is pinned without
//! game data.

use material_bin::{DEFINITION, MAGIC, UNENCRYPTED, VERSION, decode};

/// Builds little-endian definitions.
#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    /// Appends a little-endian `u8`.
    fn u8(&mut self, value: u8) -> &mut Self {
        self.0.push(value);
        self
    }
    /// Appends a little-endian `u16`.
    fn u16(&mut self, value: u16) -> &mut Self {
        self.0.extend(value.to_le_bytes());
        self
    }
    /// Appends a little-endian `u32`.
    fn u32(&mut self, value: u32) -> &mut Self {
        self.0.extend(value.to_le_bytes());
        self
    }
    /// Appends a little-endian `u64`.
    fn u64(&mut self, value: u64) -> &mut Self {
        self.0.extend(value.to_le_bytes());
        self
    }
    /// Appends a little-endian `f32`.
    fn f32(&mut self, value: f32) -> &mut Self {
        self.0.extend(value.to_le_bytes());
        self
    }
    /// Appends a `u32`-length-prefixed string.
    fn string(&mut self, value: &str) -> &mut Self {
        self.u32(value.len() as u32);
        self.0.extend(value.as_bytes());
        self
    }
}

/// A bgfx fragment binary carrying `source` and one uniform.
fn blob(source: &str) -> Vec<u8> {
    let mut blob = Writer::default();
    blob.0.extend(b"FSH\x05");
    blob.u32(0x1234).u16(1).u8(5);
    blob.0.extend(b"Tint1");
    blob.u8(2).u8(1).u16(0xffff).u16(1);
    blob.u32(source.len() as u32);
    blob.0.extend(source.as_bytes());
    blob.u8(0).u8(0).u16(16);
    blob.0
}

/// A definition with one sampler, one uniform and a pass of two variants, the second
/// carrying `source`.
fn definition(source: &str) -> Vec<u8> {
    let mut w = Writer::default();
    w.u64(MAGIC)
        .string(DEFINITION)
        .u64(VERSION)
        .u32(UNENCRYPTED);
    w.string("Synthetic").u8(0);
    // One sampler with a default texture.
    w.u8(1)
        .string("Albedo")
        .u16(0)
        .u8(1)
        .u8(0)
        .u8(0)
        .u8(0)
        .string("float");
    w.u32(1).u8(0).u8(0).u8(1).string("white").u8(0).u8(0);
    // One vec4 uniform with a default, then no overrides.
    w.u16(1).string("Tint").u16(2).u32(1).u8(1);
    for value in [1.0, 0.5, 0.25, 0.0] {
        w.f32(value);
    }
    w.u16(0);
    // One pass with a default flag and two variants.
    w.u16(1)
        .string("Opaque")
        .string("1111")
        .string("")
        .u8(1)
        .u16(3);
    w.u16(1).string("Glint").string("Off").u32(7).u16(2);
    for (glint, text) in [("Off", "void main() {}"), ("On", source)] {
        w.u8(1).u16(1).u16(1).string("Glint").string(glint);
        w.string("Fragment").string("ESSL_310").u8(1).u8(8);
        w.u16(1)
            .string("texcoord0")
            .u8(1)
            .u8(7)
            .u8(0)
            .u8(0)
            .u8(1)
            .u8(2)
            .u8(0);
        let blob = blob(text);
        w.u64(0xfeed).u32(blob.len() as u32);
        w.0.extend(blob);
    }
    w.u64(MAGIC);
    w.0
}

#[test]
fn a_synthetic_definition_decodes_every_field() {
    let source = "#version 310 es\nvoid main() { gl_FragColor = vec4(1.0); }\n";
    let material = decode(&definition(source)).unwrap();
    assert_eq!(material.name, "Synthetic");
    assert_eq!(
        material.buffers[0].default_texture.as_deref(),
        Some("white")
    );
    assert_eq!(
        material.uniforms[0].default,
        Some(vec![1.0, 0.5, 0.25, 0.0])
    );
    let pass = &material.passes[0];
    assert_eq!(
        (pass.name.as_str(), pass.blend_mode, pass.variants.len()),
        ("Opaque", Some(3), 2)
    );
    assert_eq!(pass.tag, 7);
    let glint = &pass.variants[1];
    assert_eq!(glint.flag(pass, "Glint"), Some("On"));
    let shader = &glint.shaders[0];
    assert_eq!(
        (shader.stage.as_str(), shader.platform.as_str()),
        ("Fragment", "ESSL_310")
    );
    assert_eq!(shader.inputs, ["texcoord0"]);
    assert_eq!(shader.source().unwrap(), source);
}

#[test]
fn truncated_encrypted_and_foreign_definitions_are_refused() {
    let bytes = definition("x");
    for end in [0, 8, 40, bytes.len() / 2, bytes.len() - 1] {
        assert!(decode(&bytes[..end]).is_err(), "truncated at {end}");
    }
    let mut encrypted = bytes.clone();
    let tag = 8 + 4 + DEFINITION.len() + 8;
    encrypted[tag..tag + 4].copy_from_slice(b"SMPL");
    let error = decode(&encrypted).unwrap_err().to_string();
    assert!(error.contains("encrypted"), "{error}");
    let mut other = bytes;
    other[8 + 4 + DEFINITION.len()] = 24;
    assert!(decode(&other).unwrap_err().to_string().contains("version"));
}
