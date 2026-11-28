//! Retained catalog artwork and ordered page bindings for the first-person hand.
use super::*;

/// One immutable catalog page, retained until its source or texture-view inputs change.
pub(super) struct CatalogPage {
    pub(super) _texture: Texture,
    pub(super) view: TextureView,
    pixels: render_model::PagePixels,
    pub(super) size: [u32; 3],
    format: TextureFormat,
    pub(super) raw_views: crate::actor::gpu::ArtworkRawViews,
}

/// Adjacent instances keep publication order while selecting one artwork page and layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DrawSpan {
    pub(super) instances: std::ops::Range<u32>,
    pub(super) catalog_page: Option<usize>,
    pub(super) catalog_layer: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TextureRoute {
    Legacy,
    Catalog { page: usize, layer: u32 },
}

/// Sparse retained pages share a validated route between native and shader-pack consumers.
#[derive(Default)]
pub(super) struct HandArtworkRouting {
    pub(super) pages: Vec<Option<CatalogPage>>,
    pub(super) draw_spans: Vec<DrawSpan>,
    failed: Vec<Option<PageUploadFailure>>,
}

/// One rejected page request, keyed by its source storage so a new revision retries at once.
struct PageUploadFailure {
    pixels: render_model::PagePixels,
    size: [u32; 3],
    format: TextureFormat,
    raw_views: crate::actor::gpu::ArtworkRawViews,
    attempts: u8,
    last_revision: u64,
    retry_revision: u64,
}

impl PageUploadFailure {
    /// Whether the current request still names this rejected source and texture view.
    fn matches(
        &self,
        page: &render_model::ActorTexturePage,
        size: [u32; 3],
        format: TextureFormat,
        raw_views: crate::actor::gpu::ArtworkRawViews,
    ) -> bool {
        self.pixels.ptr_eq(&page.rgba8)
            && self.size == size
            && self.format == format
            && self.raw_views == raw_views
    }
}

/// Backs off a rejected page for a bounded number of frames, then retries for free memory.
fn remember_upload_failure(
    slot: &mut Option<PageUploadFailure>,
    page: &render_model::ActorTexturePage,
    size: [u32; 3],
    format: TextureFormat,
    raw_views: crate::actor::gpu::ArtworkRawViews,
    revision: u64,
) {
    let unchanged = slot
        .as_ref()
        .is_some_and(|failure| failure.matches(page, size, format, raw_views));
    let attempts = if unchanged {
        slot.as_ref().map_or(0, |failure| failure.attempts.saturating_add(1))
    } else {
        0
    };
    let wait = 1u64 << attempts.min(4);
    *slot = Some(PageUploadFailure {
        pixels: page.rgba8.clone(),
        size,
        format,
        raw_views,
        attempts,
        last_revision: revision,
        retry_revision: revision.saturating_add(wait),
    });
}
impl HandArtworkRouting {
    /// Uploads only demanded pages; unchanged frames do not join pixels or allocate.
    pub(super) fn upload(
        &mut self,
        frame: &HandRigFrame,
        device: &RenderDevice,
        queue: &RenderQueue,
        format: TextureFormat,
        raw_views: crate::actor::gpu::ArtworkRawViews,
    ) -> bool {
        let source = frame
            .artwork
            .as_ref()
            .map(|a| a.pages.as_ref())
            .unwrap_or(&[]);
        let mut changed = self.pages.len() != source.len() || self.failed.len() != source.len();
        self.pages.resize_with(source.len(), || None);
        self.failed.resize_with(source.len(), || None);
        for (index, slot) in self.pages.iter_mut().enumerate() {
            let page = &source[index];
            if slot.as_ref().is_some_and(|current| {
                !current.pixels.ptr_eq(&page.rgba8)
                    || current.size != [u32::from(page.width), u32::from(page.height), page.layers]
                    || current.format != format
                    || current.raw_views != raw_views
            }) {
                *slot = None;
                self.failed[index] = None;
                changed = true;
            }
        }
        for location in frame.draw_locations.iter().flatten() {
            let Some(index) = usize::from(location.page).checked_sub(1) else {
                continue;
            };
            let Some(page) = source.get(index) else {
                continue;
            };
            if location.layer >= page.layers || self.pages[index].is_some() {
                continue;
            }
            let size = [u32::from(page.width), u32::from(page.height), page.layers];
            let (backed_off, stale) = match self.failed[index].as_ref() {
                Some(failure) if failure.matches(page, size, format, raw_views) => (
                    failure.last_revision == frame.revision || frame.revision < failure.retry_revision,
                    false,
                ),
                Some(_) => (false, true),
                None => (false, false),
            };
            if stale {
                self.failed[index] = None;
            }
            if backed_off {
                continue;
            }
            let bytes = usize::from(page.width)
                .checked_mul(usize::from(page.height))
                .and_then(|n| n.checked_mul(page.layers as usize))
                .and_then(|n| n.checked_mul(4));
            let limits = device.limits();
            if size.contains(&0)
                || bytes != Some(page.rgba8.len())
                || size[0] > limits.max_texture_dimension_2d
                || size[1] > limits.max_texture_dimension_2d
                || size[2] > limits.max_texture_array_layers
            {
                remember_upload_failure(
                    &mut self.failed[index],
                    page,
                    size,
                    format,
                    raw_views,
                    frame.revision,
                );
                continue;
            }
            let Some(texture) = crate::gpu_allocation::texture_with_data(
                device,
                queue,
                &TextureDescriptor {
                    label: Some("first-person catalog artwork page"),
                    size: Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: size[2],
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    view_formats: if format.is_srgb() {
                        raw_views.view_formats()
                    } else {
                        &[]
                    },
                },
                &page.rgba8.joined(),
            ) else {
                remember_upload_failure(
                    &mut self.failed[index],
                    page,
                    size,
                    format,
                    raw_views,
                    frame.revision,
                );
                continue;
            };
            let Some(view) = crate::gpu_allocation::checked(
                device,
                "first-person catalog artwork view",
                0,
                || {
                    texture.create_view(&TextureViewDescriptor {
                        label: Some("first-person catalog artwork layers"),
                        dimension: Some(TextureViewDimension::D2Array),
                        ..default()
                    })
                },
            ) else {
                remember_upload_failure(
                    &mut self.failed[index],
                    page,
                    size,
                    format,
                    raw_views,
                    frame.revision,
                );
                continue;
            };
            self.pages[index] = Some(CatalogPage {
                _texture: texture,
                view,
                pixels: page.rgba8.clone(),
                size,
                format,
                raw_views,
            });
            self.failed[index] = None;
            changed = true;
        }
        changed
    }
    /// Resolves one draw without substituting skin for a missing catalog reference.
    pub(super) fn route(&self, frame: &HandRigFrame, index: usize) -> Option<TextureRoute> {
        let Some(location) = frame.draw_locations.get(index).copied().flatten() else {
            return Some(TextureRoute::Legacy);
        };
        if location.page == 0 {
            return (location.layer == 0).then_some(TextureRoute::Legacy);
        }
        let page = usize::from(location.page) - 1;
        let resident = self.pages.get(page)?.as_ref()?;
        (location.layer < resident.size[2]).then_some(TextureRoute::Catalog {
            page,
            layer: location.layer,
        })
    }
    /// Rebuilds ordered instance spans in retained capacity; never sorts controller layers.
    pub(super) fn plan(&mut self, frame: &HandRigFrame) {
        self.draw_spans.clear();
        for index in 0..frame.rig.instances.len() {
            let Some(route) = self.route(frame, index) else {
                continue;
            };
            let (catalog_page, catalog_layer) = match route {
                TextureRoute::Legacy => (None, None),
                TextureRoute::Catalog { page, layer } => (Some(page), Some(layer)),
            };
            let index = index as u32;
            if let Some(last) = self.draw_spans.last_mut()
                && last.catalog_page == catalog_page
                && last.catalog_layer == catalog_layer
                && last.instances.end == index
            {
                last.instances.end += 1;
            } else {
                self.draw_spans.push(DrawSpan {
                    instances: index..index + 1,
                    catalog_page,
                    catalog_layer,
                });
            }
        }
    }
}

pub(super) struct CatalogBindGroup {
    pub(super) page: usize,
    pub(super) layer: u32,
    pub(super) bind_group: BindGroup,
    _layer: Buffer,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct CatalogLayerUniform {
    pub(super) layer: u32,
    pub(super) enabled: u32,
    pub(super) padding: [u32; 2],
}

/// Builds the skin binding and retained per-page/per-layer overrides.
pub(super) fn build_bind_group(
    gpu: &mut HandRigGpu,
    device: &RenderDevice,
    queue: &RenderQueue,
    cache: &PipelineCache,
) {
    if gpu.bind_group.is_none() {
        gpu.catalog_bind_groups.clear();
    }
    let current = if gpu.layout.entries.iter().any(|entry| entry.binding == 5) {
        gpu.current_bones.as_ref()
    } else {
        gpu.previous_bones.as_ref()
    };
    let (
        Some(instances),
        Some(vertices),
        Some(spans),
        Some(previous),
        Some(current),
        Some(skin),
        Some(glint),
    ) = (
        gpu.instances.as_ref(),
        gpu.vertices.buffer(),
        gpu.spans.as_ref(),
        gpu.previous_bones.as_ref(),
        current,
        gpu.skin.as_ref(),
        gpu.glint.as_ref(),
    )
    else {
        return;
    };
    let layout = cache.get_bind_group_layout(&gpu.layout);
    let make = |selected: &TextureView, layer: &Buffer, label| {
        let mut entries = Vec::from(
            bindings::HandFragmentBindings {
                instances,
                skins: selected,
                skin_sampler: &gpu.sampler,
                material: &gpu.material,
                light: &gpu.light_uniform,
                main_atlas: gpu.atlases[0]
                    .as_ref()
                    .map_or(&skin.view, |atlas| &atlas.view),
                offhand_atlas: gpu.atlases[1]
                    .as_ref()
                    .map_or(&skin.view, |atlas| &atlas.view),
                map_background: gpu.atlases[2]
                    .as_ref()
                    .map_or(&skin.view, |atlas| &atlas.view),
                item_glint: &glint.view,
                glint_sampler: &gpu.glint_sampler,
            }
            .entries(),
        );
        entries.extend(
            [
                (0, &gpu.view_uniform),
                (2, vertices),
                (3, spans),
                (4, previous),
                (5, current),
                (15, layer),
            ]
            .map(|(binding, buffer)| BindGroupEntry {
                binding,
                resource: buffer.as_entire_binding(),
            }),
        );
        entries.retain(|entry| {
            gpu.layout
                .entries
                .iter()
                .any(|kept| kept.binding == entry.binding)
        });
        device.create_bind_group(label, &layout, &entries)
    };
    if gpu.bind_group.is_none() {
        gpu.bind_group = Some(make(
            &skin.view,
            &gpu.catalog_layer_default,
            "first-person rig skin bind group",
        ));
    }
    for span in &gpu.artwork_routing.draw_spans {
        let (Some(page), Some(layer)) = (span.catalog_page, span.catalog_layer) else {
            continue;
        };
        if gpu
            .catalog_bind_groups
            .iter()
            .any(|group| group.page == page && group.layer == layer)
        {
            continue;
        }
        let selector = CatalogLayerUniform {
            layer,
            enabled: 1,
            padding: [0; 2],
        };
        let contents = bytemuck::bytes_of(&selector);
        let Some(layer_buffer) = crate::gpu_allocation::buffer_with_data(
            device,
            queue,
            "first-person catalog layer selector",
            contents,
            BufferUsages::UNIFORM,
        ) else {
            continue;
        };
        let Some(page_texture) = gpu.artwork_routing.pages.get(page).and_then(Option::as_ref)
        else {
            continue;
        };
        let bind_group = make(
            &page_texture.view,
            &layer_buffer,
            "first-person rig catalog bind group",
        );
        gpu.catalog_bind_groups.push(CatalogBindGroup {
            page,
            layer,
            bind_group,
            _layer: layer_buffer,
        });
    }
}
