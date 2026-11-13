//! Remembers the first error one module logs, so a failure report can name it after later
//! errors and panics have buried it.

use std::{
    fmt::{self, Write as _},
    sync::{Arc, OnceLock},
};

use tracing::{
    Event, Level, Subscriber,
    field::{Field, Visit},
};
use tracing_log::NormalizeEvent;
use tracing_subscriber::layer::{Context, Layer};

use super::CallerLayer;

/// The first `ERROR` event one target logged since its layer was installed.
#[derive(Clone, Debug, Default)]
pub struct FirstError(Arc<OnceLock<String>>);

impl FirstError {
    /// The event's message followed by its fields, or `None` while the target has logged no
    /// error.
    pub fn get(&self) -> Option<&str> {
        self.0.get().map(String::as_str)
    }
}

/// A caller layer for [`super::Options::layers`] and the handle it records into: the first
/// `ERROR` event whose target is exactly `target`. Later errors from that target change nothing.
pub fn first_error(target: impl Into<String>) -> (CallerLayer, FirstError) {
    let first = FirstError::default();
    let layer = FirstErrorLayer {
        target: target.into(),
        first: first.clone(),
    };
    (Box::new(layer), first)
}

/// Records the first matching event; see [`first_error`].
struct FirstErrorLayer {
    target: String,
    first: FirstError,
}

impl<S: Subscriber> Layer<S> for FirstErrorLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() != Level::ERROR || self.first.0.get().is_some() {
            return;
        }
        // `log`-crate records keep their original target in normalized metadata.
        let normalized = event.normalized_metadata();
        let metadata = normalized.as_ref().unwrap_or_else(|| event.metadata());
        if metadata.target() != self.target {
            return;
        }
        let mut text = Text(String::new());
        event.record(&mut text);
        let _ = self.first.0.set(text.0);
    }
}

/// Formats an event as its message, then ` name=value` for each other field.
struct Text(String);

impl Text {
    /// Appends one field; the message comes first because `tracing` records it first.
    fn field(&mut self, field: &Field, value: fmt::Arguments<'_>) {
        match field.name() {
            "message" => {
                let _ = write!(self.0, "{value}");
            }
            name if name.starts_with("log.") => {}
            name => {
                let _ = write!(self.0, " {name}={value}");
            }
        }
    }
}

impl Visit for Text {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.field(field, format_args!("{value}"));
        } else {
            self.field(field, format_args!("{value:?}"));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.field(field, format_args!("{value:?}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    /// Later errors, other targets and lower levels never replace the first error.
    #[test]
    fn only_the_first_error_of_the_target_is_kept() {
        let (layer, first) = first_error("gpu::callbacks");
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::error!(target: "gpu::callbacks::nested", "another module");
            tracing::warn!(target: "gpu::callbacks", "only a warning");
            assert_eq!(first.get(), None);
            tracing::error!(target: "gpu::callbacks", code = 7_u32, "device lost: {}", "reset");
            tracing::error!(target: "gpu::callbacks", "surface is not configured");
        });
        assert_eq!(first.get(), Some("device lost: reset code=7"));
    }
}
