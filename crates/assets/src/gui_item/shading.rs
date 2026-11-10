//! Face lighting for block item GUI geometry.

use crate::{MODEL_TEMPLATE_FLAG_GUI_WORLD_SHADE, ModelTemplate, VisualKind};

use super::block_model::face_brightness;

/// World flat shading of upward faces.
pub const WORLD_SHADE_UP: f32 = 1.0;
/// World flat shading of downward faces.
pub const WORLD_SHADE_DOWN: f32 = 0.5;
/// World flat shading of north and south faces.
pub const WORLD_SHADE_Z: f32 = 0.8;
/// World flat shading of west and east faces.
pub const WORLD_SHADE_X: f32 = 0.6;

/// How a block item's GUI faces are lit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuiShading {
    /// Full texture brightness for inventory artwork without directional face dimming.
    Unshaded,
    /// The inventory cube's Up, South and West brightness, chosen by each face's axis.
    Inventory,
    /// Flat world shading by outward direction, for shapes whose item GUI draws through
    /// their world tessellator.
    World,
}

impl GuiShading {
    /// The shading of a block item's GUI geometry: world shading when its item template
    /// carries [`MODEL_TEMPLATE_FLAG_GUI_WORLD_SHADE`], otherwise the inventory cube's.
    #[must_use]
    pub fn of(kind: VisualKind, template: Option<u32>, templates: &[ModelTemplate]) -> Self {
        let world = kind == VisualKind::Model
            && template
                .map(|template| crate::model::item_model_template(templates, template))
                .and_then(|template| templates.get(template as usize))
                .is_some_and(|template| template.flags & MODEL_TEMPLATE_FLAG_GUI_WORLD_SHADE != 0);
        if world { Self::World } else { Self::Inventory }
    }

    /// Brightness of a quad with outward counter-clockwise `corners`, in block units.
    #[must_use]
    pub fn brightness(self, corners: [[f32; 3]; 4]) -> f32 {
        match self {
            Self::Unshaded => 1.0,
            Self::Inventory => face_brightness(corners),
            Self::World => {
                let [a, b, c] = [corners[0], corners[1], corners[2]];
                let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let normal = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                let [x, y, z] = normal.map(f32::abs);
                if y >= x && y >= z {
                    if normal[1] >= 0.0 {
                        WORLD_SHADE_UP
                    } else {
                        WORLD_SHADE_DOWN
                    }
                } else if x >= z {
                    WORLD_SHADE_X
                } else {
                    WORLD_SHADE_Z
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
