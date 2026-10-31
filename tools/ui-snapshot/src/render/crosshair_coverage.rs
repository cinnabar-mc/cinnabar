//! Contrasting backdrops expose opaque cursor backgrounds and partial coverage.

use super::{Error, sprite_outline::rect};
use image::ImageEncoder;
use jsonui_editor::raster::{Canvas, Page};
use std::{path::Path, sync::Arc};
use ui::{UiNode, UiNodeId, UiTree, UiVisual};

/// Captures original cursor art, or a caller-supplied pack's icons-sheet crop.
pub(super) fn png(size: [u32; 2], fixed: bool, pack: Option<&Path>) -> Result<Vec<u8>, Error> {
    let role = assets::HudTextureRole::Crosshair;
    let image = if let Some(pack) = pack {
        let image = image::open(pack.join(role.source_path()))
            .map_err(|error| Error::Invalid(error.to_string()))?
            .to_rgba8();
        let [w, h] = assets::HUD_ICONS_SHEET_SIZE;
        let [x, y, cw, ch] = role.source_crop().unwrap();
        image::imageops::crop_imm(
            &image,
            x * image.width() / w,
            y * image.height() / h,
            cw * image.width() / w,
            ch * image.height() / h,
        )
        .to_image()
    } else {
        let [width, height] = role.expected_size();
        image::RgbaImage::from_fn(width, height, |x, y| {
            let bright = (x == 7 && (3..12).contains(&y)) || (y == 7 && (3..12).contains(&x));
            image::Rgba(if bright { [255; 4] } else { [0, 0, 0, 255] })
        })
    };
    let (w, h) = image.dimensions();
    let pages = [
        Page::white(),
        Page {
            width: w,
            height: h,
            rgba: Arc::from(image.into_raw()),
            repeat_content: None,
        },
    ];
    let mut nodes = Vec::new();
    for (column, background) in [[40, 90, 140, 255], [190, 170, 130, 255], [80, 140, 90, 255]]
        .into_iter()
        .enumerate()
    {
        let left = column as f32 * size[0] as f32 / 3.;
        let right = (column + 1) as f32 * size[0] as f32 / 3.;
        nodes.push(
            UiNode::new(
                UiNodeId::new(nodes.len() as u32),
                None,
                rect([left, 0., right, size[1] as f32])?,
            )
            .with_visual(UiVisual::Solid {
                texture_page: 0,
                color: background,
            }),
        );
        let x = (left + right) / 2.;
        let uv = [0, 0, w as u16, h as u16];
        let visual = if fixed {
            UiVisual::RgbCoverageSprite {
                texture_page: 1,
                uv,
                color: [255; 4],
            }
        } else {
            UiVisual::Sprite {
                texture_page: 1,
                uv,
                color: [255; 4],
            }
        };
        nodes.push(
            UiNode::new(
                UiNodeId::new(nodes.len() as u32),
                None,
                rect([x - 45., 75., x + 45., 165.])?,
            )
            .with_visual(visual),
        );
    }
    let draw = UiTree::new(nodes)
        .map_err(|e| Error::Invalid(format!("{e:?}")))?
        .build_draw_list()
        .map_err(|e| Error::Invalid(format!("{e:?}")))?;
    let mut canvas = Canvas::new(size[0], size[1]);
    canvas.draw(&draw, |page| pages.get(usize::from(page)));
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &canvas.into_rgba(),
            size[0],
            size[1],
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| Error::Invalid(e.to_string()))?;
    Ok(png)
}
