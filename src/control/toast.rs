use gtk::prelude::*;

pub trait GlobalToast {
    fn toast(&self, message: impl Into<String>);

    fn add_toast_inner(&self, toast: adw::Toast);
}

impl<T> GlobalToast for T
where
    T: IsA<gtk::Widget>,
{
    fn toast(&self, message: impl Into<String>) {
        let toast = adw::Toast::builder()
            .timeout(2)
            .use_markup(false)
            .title(message.into())
            .build();
        self.add_toast_inner(toast);
    }

    fn add_toast_inner(&self, toast: adw::Toast) {
        if let Some(overlay) = self
            .ancestor(adw::ToastOverlay::static_type())
            .and_downcast::<adw::ToastOverlay>()
        {
            overlay.add_toast(toast);
        } else {
            panic!("Trying to display a toast when the parent doesn't support it");
        }
    }
}
