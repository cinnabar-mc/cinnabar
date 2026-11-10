//! Derived atmosphere values are prepared once per upload, after every environment override.

use bevy::{math::Vec4, render::render_resource::ShaderType};
use render_scene::AtmosphereFrame;

/// Preserves the shared atmosphere prefix and appends the native fog colour in encoded RGB.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, ShaderType)]
pub(crate) struct AtmosphereUniform {
    frame: AtmosphereFrame,
    fog_gamma: Vec4,
}

impl From<AtmosphereFrame> for AtmosphereUniform {
    /// Derives encoded fog from the final frame so weather, media and server overrides agree.
    fn from(frame: AtmosphereFrame) -> Self {
        let fog_gamma = render_model::celestial::rgb_to_gamma(frame.fog_color());
        Self {
            frame,
            fog_gamma: Vec4::from_array([fog_gamma[0], fog_gamma[1], fog_gamma[2], 0.0]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_uniform_keeps_the_frame_prefix_and_encodes_its_final_fog() {
        for time in [0.0, 6_000.0, 12_000.0, 18_000.0] {
            for weather in [0.0, 0.5, 1.0] {
                let frame = AtmosphereFrame::from_bedrock_time(time, weather, weather);
                let uniform = AtmosphereUniform::from(frame);
                let bytes = bytemuck::bytes_of(&uniform);
                let prefix = bytemuck::bytes_of(&frame);
                assert_eq!(&bytes[..prefix.len()], prefix);
                assert_eq!(
                    uniform.fog_gamma.truncate().to_array(),
                    render_model::celestial::rgb_to_gamma(frame.fog_color())
                );
                let mut encoded =
                    bevy::render::render_resource::encase::UniformBuffer::new(Vec::<u8>::new());
                encoded.write(&uniform).unwrap();
                assert_eq!(encoded.into_inner(), bytes);
            }
        }
    }
}
