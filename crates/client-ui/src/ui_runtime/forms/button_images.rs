//! Logs the image reference each menu button carries, so a blank button icon can be traced
//! to its pack path or download.

use protocol::{FormButtonImage, MenuElement, ServerFormModel};

/// One menu button: its response index, literal label and image reference.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ButtonImageRow<'a> {
    pub index: usize,
    pub text: &'a str,
    pub image: Option<&'a FormButtonImage>,
}

/// Every button of a menu form in response order; other form kinds have none.
pub(super) fn button_image_rows(model: &ServerFormModel) -> Vec<ButtonImageRow<'_>> {
    match model {
        ServerFormModel::TextMenu(form) => form
            .buttons
            .iter()
            .enumerate()
            .map(|(index, text)| ButtonImageRow {
                index,
                text,
                image: form.button_images.get(index).and_then(Option::as_ref),
            })
            .collect(),
        ServerFormModel::ElementMenu(form) => form
            .elements
            .iter()
            .filter_map(|element| match element {
                MenuElement::Button { text, image } => Some((&**text, image.as_ref())),
                _ => None,
            })
            .enumerate()
            .map(|(index, (text, image))| ButtonImageRow { index, text, image })
            .collect(),
        _ => Vec::new(),
    }
}

/// Logs one line per menu button with its image source kind and reference.
pub(super) fn log(form_id: u32, model: &ServerFormModel) {
    if !tracing::enabled!(target: "server_form", tracing::Level::DEBUG) {
        return;
    }
    for row in button_image_rows(model) {
        let (source, reference) = match row.image {
            Some(FormButtonImage::Path(path)) => ("path", &**path),
            Some(FormButtonImage::Url(url)) => ("url", &**url),
            None => ("none", ""),
        };
        tracing::debug!(
            target: "server_form",
            form_id,
            index = row.index,
            text = ?row.text,
            source,
            reference,
            "form button image"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::{ElementMenuForm, FormText, TextMenuForm};

    use super::*;

    /// Creates a literal button label for a form fixture.
    fn text(value: &str) -> FormText {
        FormText::Literal(Arc::from(value))
    }

    #[test]
    fn text_menu_rows_pair_each_button_with_its_image() {
        let model = ServerFormModel::TextMenu(TextMenuForm {
            title: text("Games"),
            content: text(""),
            buttons: Arc::from([text("Sky"), text("Hub")]),
            button_images: Arc::from([
                Some(FormButtonImage::Url(Arc::from(
                    "https://example.invalid/sky.png",
                ))),
                None,
            ]),
            omitted_images: 1,
        });
        let url = FormButtonImage::Url(Arc::from("https://example.invalid/sky.png"));
        assert_eq!(
            button_image_rows(&model),
            vec![
                ButtonImageRow {
                    index: 0,
                    text: "Sky",
                    image: Some(&url),
                },
                ButtonImageRow {
                    index: 1,
                    text: "Hub",
                    image: None,
                },
            ]
        );
    }

    #[test]
    fn element_menu_rows_count_buttons_only() {
        let path = FormButtonImage::Path(Arc::from("textures/ui/hub"));
        let model = ServerFormModel::ElementMenu(ElementMenuForm {
            title: text("Games"),
            content: text(""),
            elements: Arc::from([
                MenuElement::Header(text("Teleport")),
                MenuElement::Button {
                    text: text("Hub"),
                    image: Some(path.clone()),
                },
            ]),
        });
        assert_eq!(
            button_image_rows(&model),
            vec![ButtonImageRow {
                index: 0,
                text: "Hub",
                image: Some(&path),
            }]
        );
    }
}
