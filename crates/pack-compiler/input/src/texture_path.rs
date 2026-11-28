use std::path::{Path, PathBuf};

use assets::AssetError;
use image::ImageFormat;

/// Native extension priority for an extensionless resource reference.
pub const IMAGE_EXTENSIONS: [(&str, ImageFormat); 4] = [
    ("tga", ImageFormat::Tga),
    ("png", ImageFormat::Png),
    ("jpg", ImageFormat::Jpeg),
    ("jpeg", ImageFormat::Jpeg),
];

/// The format of pack image bytes, as the vanilla loader detects it: content wins, and TGA,
/// which carries no magic number, is the fallback. A `.tga` path may hold PNG data, and a
/// `.png` path may hold TGA data.
#[must_use]
pub fn detect_image_format(bytes: &[u8]) -> ImageFormat {
    image::guess_format(bytes).unwrap_or(ImageFormat::Tga)
}

/// Selects an explicit path or the highest-priority supported static image.
pub fn static_texture_path(root: &Path, source: &str, key: &str) -> Result<PathBuf, AssetError> {
    if Path::new(source).extension().is_some() {
        return Ok(root.join(source));
    }
    for (extension, format) in IMAGE_EXTENSIONS {
        if !matches!(format, ImageFormat::Png | ImageFormat::Tga) {
            continue;
        }
        let path = root.join(format!("{source}.{extension}"));
        if path.try_exists().map_err(|source| AssetError::TextureIo {
            key: key.into(),
            path: path.clone(),
            source,
        })? {
            return Ok(path);
        }
    }
    Ok(root.join(format!("{source}.png")))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn image_format_follows_content_and_falls_back_to_tga() {
        let mut png = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 255]))
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        assert_eq!(detect_image_format(png.get_ref()), ImageFormat::Png);
        assert_eq!(detect_image_format(b"not an image"), ImageFormat::Tga);
    }

    #[test]
    fn extensionless_images_choose_tga_and_explicit_png_remains_explicit() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("grass.png"), b"png").unwrap();
        std::fs::write(root.path().join("grass.tga"), b"tga").unwrap();
        assert_eq!(
            static_texture_path(root.path(), "grass", "grass").unwrap(),
            root.path().join("grass.tga")
        );
        assert_eq!(
            static_texture_path(root.path(), "grass.png", "grass").unwrap(),
            root.path().join("grass.png")
        );
    }
}
