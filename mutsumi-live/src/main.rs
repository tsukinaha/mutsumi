use adw::prelude::*;
use mutsumi::{PlayParams, PlaySource};
use mutsumi_live::MutsumiLiveWindow;


fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let app = adw::Application::builder()
        .application_id("io.github.mutsumi-live.example.player")
        .build();

    app.connect_activate(move |app| {
        mutsumi_live::init();
        mutsumi::init();

        let window = MutsumiLiveWindow::new(app);
        window.set_title(Some("Mutsumi Player"));
        window.set_default_width(1280);
        window.set_default_height(720);
        window.present();
    });

    app.run_with_args::<&str>(&[]);
}
