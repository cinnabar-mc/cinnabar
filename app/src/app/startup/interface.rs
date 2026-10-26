//! Prepares immutable menu presentation while the main thread opens the renderer.

use super::*;
use client_ui::ui_runtime::presentation::UiPresentationRuntime;

/// Presentation owners that must be ready before the event loop starts.
pub(in crate::app) struct Interface {
    pub presentation: UiPresentationRuntime,
    pub catalog: crate::runtime::network::PackUiCatalog,
    pub block_entity_font: Arc<assets::RuntimeFontCatalog>,
}

struct Prepared {
    interface: Interface,
    world: world::WorldUi,
}

/// A finite worker joined by main before required UI failures can enter the event loop.
pub(in crate::app) struct PreparingInterface(Option<std::thread::JoinHandle<Result<Prepared>>>);

impl PreparingInterface {
    /// Starts CPU-only UI work without borrowing the application or its GPU device.
    pub(in crate::app) fn start(
        menu: asset_startup::MenuAssets,
        hud: LoadedHudAssets,
        icons: LoadedIconAssets,
        json_ui: PreparingJsonUi,
        path: PathBuf,
        vanilla_pack: PathBuf,
        gui_scale: Option<u8>,
    ) -> Result<Self> {
        let worker = std::thread::Builder::new()
            .name("menu-interface".into())
            .spawn(move || {
                render_scene::startup_timing::measure("interface", || {
                    prepare(menu, hud, icons, json_ui, path, vanilla_pack, gui_scale)
                })
            })
            .context("start menu interface preparation")?;
        Ok(Self(Some(worker)))
    }

    /// Reports preparation failures on main and keeps direct joins' world validation eager.
    pub(in crate::app) fn finish(
        mut self,
        input: world::WorldInput,
        direct: bool,
    ) -> Result<(Interface, world::WorldPreparation)> {
        let prepared = render_scene::startup_timing::measure("interface_wait", || {
            self.0
                .take()
                .expect("menu interface worker is joined once")
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })?;
        let world = world::WorldPreparation::new(input, prepared.world, direct)?;
        Ok((prepared.interface, world))
    }
}

impl Drop for PreparingInterface {
    /// Retires the finite worker when another startup prerequisite fails first.
    fn drop(&mut self) {
        if let Some(worker) = self.0.take() {
            let _ = worker.join();
        }
    }
}

/// Builds static UI owners off the frame, retaining validated carriers for world preparation.
fn prepare(
    menu: asset_startup::MenuAssets,
    hud: LoadedHudAssets,
    icons: LoadedIconAssets,
    json_ui: PreparingJsonUi,
    path: PathBuf,
    vanilla_pack: PathBuf,
    gui_scale: Option<u8>,
) -> Result<Prepared> {
    std::thread::scope(|scope| {
        let directory = path.parent().unwrap_or(Path::new("."));
        let artwork = scope.spawn(move || {
            render_scene::startup_timing::measure("interface_artwork_decode", || {
                oreui::images::load_optional_oreui_images(directory)
            })
        });
        let font = menu.fonts.clone().into_runtime();
        let block_entity_font = Arc::clone(&font);
        let font = render_scene::startup_timing::measure("interface_fonts", || {
            oreui::fonts::install(font, directory)
        });
        let world_icons = Arc::clone(icons.runtime());
        let mut presentation = render_scene::startup_timing::measure("interface_atlas", || {
            UiPresentationRuntime::with_hud_and_icons(
                font,
                hud.into_runtime(),
                icons.into_runtime(),
            )
        })
        .context("prepare bounded font, HUD, and item-icon texture arrays for UI rendering")?;
        render_scene::startup_timing::measure("interface_json_ui", || {
            json_ui
                .join()
                .and_then(|prepared| presentation.enable_prepared_json_ui(prepared))
        })
        .map_err(|reason| anyhow::anyhow!("JSON-UI engine failed to start: {reason}"))?;
        let catalog = crate::runtime::network::PackUiCatalog(
            presentation
                .pack_catalog_base()
                .context("JSON-UI engine is missing its carrier catalog")?,
        );
        presentation.enable_touch_controls(launcher::TOUCH_PRIMARY);
        render_scene::startup_timing::measure("interface_artwork", || {
            if let Some(images) = join(artwork)
                && let Err(reason) = presentation.enable_oreui_originals(images)
            {
                bevy::log::warn!("OreUI originals disabled ({reason})");
            }
        });
        let world = world::WorldUi {
            menu,
            icons: world_icons,
            gui: presentation.gui_model_preparation(),
            vanilla_pack,
        };
        presentation.enable_player_preview();
        presentation.set_gui_scale_preference(gui_scale);
        Ok(Prepared {
            interface: Interface {
                presentation,
                catalog,
                block_entity_font,
            },
            world,
        })
    })
}
