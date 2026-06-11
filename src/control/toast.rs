use gtk::prelude::*;

pub trait GlobalToast {
    fn toast(&self, message: impl Into<String>);
}

impl<T> GlobalToast for T
where
    T: IsA<gtk::Widget>,
{
    fn toast(&self, message: impl Into<String>) {
        let message = message.into();

        let Some(overlay) = self
            .ancestor(adw::ToastOverlay::static_type())
            .and_downcast::<adw::ToastOverlay>()
        else {
            tracing::warn!("No ToastOverlay ancestor, dropping toast: {message}");
            return;
        };

        let toast = adw::Toast::builder()
            .timeout(2)
            .use_markup(false)
            .title(message)
            .build();
        overlay.add_toast(toast);
    }
}
