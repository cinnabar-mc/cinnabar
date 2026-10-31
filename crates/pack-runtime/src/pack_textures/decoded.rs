//! Strip frame operations on decoded source pixels.

use super::DecodedTexture;

impl DecodedTexture {
    /// Keeps the upper-left square frame of an integral strip on either axis.
    /// Square images, other rectangles, and malformed pixel buffers remain unchanged.
    pub fn retain_first_strip_frame(&mut self) {
        let side = self.width.min(self.height);
        let extent = self.width.max(self.height);
        if side == 0
            || side == extent
            || !extent.is_multiple_of(side)
            || self
                .width
                .checked_mul(self.height)
                .and_then(|pixels| pixels.checked_mul(4))
                .and_then(|bytes| usize::try_from(bytes).ok())
                != Some(self.rgba8.len())
        {
            return;
        }
        let frame_bytes = (side * side * 4) as usize;
        if self.width == side {
            let mut pixels = std::mem::take(&mut self.rgba8).into_vec();
            pixels.truncate(frame_bytes);
            self.rgba8 = pixels.into_boxed_slice();
        } else {
            let mut pixels = Vec::with_capacity(frame_bytes);
            for row in self.rgba8.chunks_exact(self.width as usize * 4) {
                pixels.extend_from_slice(&row[..side as usize * 4]);
            }
            self.rgba8 = pixels.into_boxed_slice();
        }
        self.width = side;
        self.height = side;
    }

    /// Downscales the texture so neither dimension exceeds `max_side`, maintaining aspect ratio.
    /// Does nothing if the texture is already within bounds.
    pub fn downscale_to_max(&mut self, max_side: u32) {
        if self.width <= max_side && self.height <= max_side {
            return;
        }
        if self.width == 0 || self.height == 0 || max_side == 0 {
            return;
        }
        let max_dim = self.width.max(self.height) as f32;
        let scale = max_side as f32 / max_dim;
        let target_w = ((self.width as f32 * scale).round() as u32).clamp(1, max_side);
        let target_h = ((self.height as f32 * scale).round() as u32).clamp(1, max_side);

        let Some(img) = image::RgbaImage::from_raw(self.width, self.height, self.rgba8.to_vec()) else {
            return;
        };
        let resized = image::imageops::resize(&img, target_w, target_h, image::imageops::FilterType::Triangle);
        self.width = target_w;
        self.height = target_h;
        self.rgba8 = resized.into_raw().into_boxed_slice();
    }
}
