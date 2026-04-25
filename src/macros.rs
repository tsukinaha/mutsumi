#[macro_export]
macro_rules! dyn_event {
    ($lvl:ident, $($arg:tt)+) => {
        match $lvl {
            ::gtk::glib::LogLevel::Debug => ::tracing::debug!($($arg)+),
            ::gtk::glib::LogLevel::Message | ::gtk::glib::LogLevel::Info => ::tracing::info!($($arg)+),
            ::gtk::glib::LogLevel::Warning => ::tracing::warn!($($arg)+),
            ::gtk::glib::LogLevel::Error | ::gtk::glib::LogLevel::Critical  => ::tracing::error!($($arg)+),
        }
    };
}
