mod contexted;
mod video;

pub use contexted::*;
use gstreamer::State;
pub use video::*;

use flume::{Receiver, Sender, unbounded};
use once_cell::sync::Lazy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GstTrackKind {
    Video,
    Audio,
    Subtitle,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GstTrackInfo {
    pub id: String,
    pub index: i32,
    pub kind: GstTrackKind,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub selected: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GstTracks {
    pub video: Vec<GstTrackInfo>,
    pub audio: Vec<GstTrackInfo>,
    pub subtitle: Vec<GstTrackInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChapterInfo {
    pub title: Option<String>,
    pub start_seconds: f64,
    pub end_seconds: Option<f64>,
}

pub type ChapterList = Vec<ChapterInfo>;

#[derive(Debug, Clone, PartialEq)]
pub enum ListenEvent {
    Seek,
    SeekDone,
    PlaybackRestart,
    Eof,
    StartFile,
    Duration(f64),
    Pause(bool),
    Buffering(i32),
    Error(String),
    Warning(String),
    TrackList(GstTracks),
    Volume(f64),
    Speed(f64),
    Shutdown,
    TimePos(f64),
    StateChanged(State),
    PausedForCache(bool),
    ChapterList(ChapterList),
    SubtitleAdded(String),
}

pub struct GstEventChannel {
    pub tx: Sender<ListenEvent>,
    pub rx: Receiver<ListenEvent>,
}

pub static GST_EVENT_CHANNEL: Lazy<GstEventChannel> = Lazy::new(|| {
    let (tx, rx) = unbounded::<ListenEvent>();
    GstEventChannel { tx, rx }
});
