use std::path::Path;

pub struct Play {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub url: String,
}

pub enum PlaySource<P>
where
    P: AsRef<Path>,
{
    Url(String),
    File(P),
}
