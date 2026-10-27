//! Texture path selection inside catalog entries.

use pack_compile_input::texture_set::TextureSetLayer;

use super::{Value, normalize_texture_path};

/// A texture entry is a path, an object with `path`, or a variation list whose
/// first element is used.
pub(super) fn first_texture_path(value: &Value) -> Option<String> {
    let path = match value {
        Value::String(path) => path.as_str(),
        Value::Object(entry) => entry.get("path")?.as_str()?,
        Value::Array(entries) => return first_texture_path(entries.first()?),
        _ => return None,
    };
    let path = normalize_texture_path(path);
    (!path.is_empty()).then(|| path.to_owned())
}

/// Finds the first indexed image candidate without reading or decoding image payloads.
/// Only admitted cached pixels may reuse it; uncached candidates still require normal decoding.
pub fn indexed_image_candidate(view: &super::LayeredPackView, path: &str) -> Option<String> {
    let path = normalize_texture_path(path);
    let Some((layer, source, definition)) = super::texture_set_definition(view, path) else {
        return indexed_image(view, path);
    };
    let candidate = match definition.color {
        TextureSetLayer::Uniform(_) => Some(source.to_ascii_lowercase()),
        TextureSetLayer::Image(name) => super::texture_set_image_paths(&source, &name)
            .into_iter()
            .find(|candidate| layer.file_size(candidate).is_some())
            .map(|candidate| candidate.to_ascii_lowercase()),
    };
    // A set whose color image is missing is discarded, as decoding discards it.
    candidate.or_else(|| indexed_image(view, path))
}

/// Uses the decoder's extension order and indexed file sizes without loading pixels.
fn indexed_image(view: &super::LayeredPackView, path: &str) -> Option<String> {
    super::texture_image_paths(path)
        .into_iter()
        .find(|path| view.layers().any(|layer| layer.file_size(path).is_some()))
        .map(|path| path.to_ascii_lowercase())
}
