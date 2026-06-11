use adw::prelude::*;
use mutsumi::PlayerPage;

const DEFAULT_URL: &str =
    "https://test-videos.co.uk/vids/bigbuckbunny/mp4/h264/1080/Big_Buck_Bunny_1080_10s_30MB.mp4";

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let app = adw::Application::builder()
        .application_id("io.github.mutsumi.example.player")
        .build();

    app.connect_activate(|app| {
        mutsumi::control_init();

        let url = std::env::args()
            .nth(1)
            .unwrap_or_else(|| DEFAULT_URL.to_string());

        let player = PlayerPage::new();
        player.set_video_title("wl-proxy embed mpv demo");
        player.set_video_subtitle(url.clone());

        let toast_overlay = adw::ToastOverlay::new();
        toast_overlay.set_child(Some(&player));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Mutsumi Player")
            .default_width(1280)
            .default_height(720)
            .content(&toast_overlay)
            .build();

        window.present();
        player.grab_focus();

        player.play(&url, 0.0);
        player.reveal_controls(true);
    });

    app.run();
}
