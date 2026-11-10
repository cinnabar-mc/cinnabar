//! Admits only catalog content with every asset present and hash-checked in the host cache.
use launcher::install_layout::InstallLayout;

/// Loads cached optional content without doing network work on the caller's thread.
pub(super) fn load(layout: &InstallLayout) {
    let root = launcher_host::cosmetics::cache_dir(layout);
    let Some(catalog) = launcher_host::cosmetics::cached(&root) else {
        return;
    };
    // Building rigs dominates menu startup with a populated catalog; registration stays ordered.
    let prepared = parallel_map(catalog.items, |item| {
        let geometry = launcher_host::cosmetics::read_asset(&root, &item.assets.geometry)?;
        let texture = launcher_host::cosmetics::read_asset(&root, &item.assets.texture)?;
        let animation = match &item.assets.animation {
            Some(asset) => launcher_host::cosmetics::read_asset(&root, asset)?,
            None => Vec::new(),
        };
        render_model::hats::prepare(item, &geometry, &texture, &animation)
    });
    let admitted: Vec<String> = prepared
        .into_iter()
        .flatten()
        .filter_map(render_model::hats::install)
        .map(|item| item.id().to_owned())
        .collect();
    cosmetics::activate(&admitted);
    let mut emotes = Vec::new();
    for item in catalog.emotes {
        let Some(bytes) = launcher_host::cosmetics::read_asset(&root, &item.clip) else {
            cosmetics::emotes::reject();
            continue;
        };
        if let Some(item) = cosmetics::emotes::Item::admit(item, &bytes) {
            emotes.push(item);
        } else {
            bevy::log::warn!("malformed catalog emote skipped");
        }
    }
    cosmetics::emotes::activate(&emotes);
}

/// Refreshes catalog files on the wardrobe worker, retaining usable cached content during outages.
#[cfg(not(any(test, feature = "test-support")))]
pub(crate) fn refresh(layout: &InstallLayout) {
    let _ = launcher_host::cosmetics::refresh(&launcher_host::cosmetics::cache_dir(layout));
    load(layout);
}

/// Maps `items` on scoped workers, one contiguous chunk per available core, keeping their order.
fn parallel_map<T: Send, R: Send>(items: Vec<T>, map: impl Fn(T) -> R + Sync) -> Vec<R> {
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(items.len());
    if workers <= 1 {
        return items.into_iter().map(map).collect();
    }
    let chunk = items.len().div_ceil(workers);
    let mut chunks = Vec::with_capacity(workers);
    let mut items = items.into_iter();
    loop {
        let next: Vec<T> = items.by_ref().take(chunk).collect();
        if next.is_empty() {
            break;
        }
        chunks.push(next);
    }
    let map = &map;
    std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| scope.spawn(move || chunk.into_iter().map(map).collect::<Vec<R>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    /// Chunked workers must return results in input order, including a short final chunk.
    #[test]
    fn parallel_map_keeps_input_order() {
        let items: Vec<u32> = (0..37).collect();
        let doubled = super::parallel_map(items, |item| item * 2);
        assert_eq!(doubled, (0..37).map(|item| item * 2).collect::<Vec<_>>());
        assert!(super::parallel_map(Vec::<u32>::new(), |item| item).is_empty());
    }
}
