//! Platform support shared by shader-pack controls and rendering.

/// Whether this platform can run the isolated shader compiler.
/// iOS needs in-process compilation before shader packs can be offered.
pub const SHADER_PACK_PLATFORM_SUPPORTED: bool = supported_on(std::env::consts::OS);

/// Reports whether the target OS supports the shader-pack compiler process.
const fn supported_on(os: &str) -> bool {
    !matches!(os.as_bytes(), b"ios")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_compiler_platform_support() {
        for (os, expected) in [
            ("ios", false),
            ("android", true),
            ("linux", true),
            ("macos", true),
            ("windows", true),
        ] {
            assert_eq!(supported_on(os), expected, "{os}");
        }
    }
}
