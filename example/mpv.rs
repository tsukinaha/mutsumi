use mutsumi::video::MPVGLArea;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box as GtkBox, Entry, Button, Orientation, Label};

fn main() {
    gtk::init().expect("Failed to initialize GTK");

    let area = MPVGLArea::new();
    let area_clone = area.clone();

    glib::spawn_future_local(async move {
        glib::timeout_future(std::time::Duration::from_secs(1)).await;
        area_clone.play("https://www.youtube.com/watch?v=IalBrXP3LVU&list=RDIalBrXP3LVU", 0.0);
    });

    let app = Application::builder()
        .application_id("org.mutsumi.example.mpvglarea")
        .build();

    let area_for_activate = area.clone();

    app.connect_activate(move |app| {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("mutsumi MPVGLArea example")
            .default_width(960)
            .default_height(540)
            .build();

        let vbox = GtkBox::new(Orientation::Vertical, 6);

        let gl_area = area_for_activate.clone();
        gl_area.set_hexpand(true);
        gl_area.set_vexpand(true);
        vbox.append(&gl_area);

        let controls = GtkBox::new(Orientation::Horizontal, 6);

        let entry = Entry::new();
        entry.set_placeholder_text(Some("file:///path/to/video.mp4 or https://..."));
        controls.append(&entry);

        let play_btn = Button::with_label("Play");
        let pause_btn = Button::with_label("Toggle Pause");
        let stop_btn = Button::with_label("Stop");

        controls.append(&play_btn);
        controls.append(&pause_btn);
        controls.append(&stop_btn);

        vbox.append(&controls);

        let gl_area_play = gl_area.clone();
        let entry_play = entry.clone();
        play_btn.connect_clicked(move |_| {
            let url = entry_play.text().to_string();
            if url.trim().is_empty() {
                eprintln!("Please enter a URL or file path to play.");
                return;
            }
            gl_area_play.play(&url, 0.0);
            eprintln!("Play requested: {}", url);
        });

        let gl_area_pause = gl_area.clone();
        pause_btn.connect_clicked(move |_| {
            gl_area_pause.pause();
            eprintln!("Toggle pause");
        });

        let gl_area_stop = gl_area.clone();
        stop_btn.connect_clicked(move |_| {
            gl_area_stop.mpv().stop();
            eprintln!("Stop requested");
        });

        window.set_child(Some(&vbox));
        window.present();
    });

    app.run();
}
