use std::io::Read;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_tungstenite::tokio::connect_async;
use async_tungstenite::tungstenite::Message;
use bililive::{Operation, Packet, Protocol};
use flate2::read::ZlibDecoder;
use flume::Sender;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};

pub struct LiveDanmaku {
    pub text: String,
    pub color: u32,
}

pub fn parse_bilibili_live_room_id(url: &str) -> Option<u64> {
    let path = url
        .trim()
        .strip_prefix("https://live.bilibili.com/")
        .or_else(|| url.trim().strip_prefix("http://live.bilibili.com/"))?;
    path.split(['?', '#', '/']).next()?.parse::<u64>().ok()
}

pub async fn check_bilibili_live_status(room_id: u64) -> Option<bool> {
    use gtk::gio;
    use gtk::prelude::FileExtManual;

    let uri = format!("https://api.live.bilibili.com/room/v1/Room/get_info?id={room_id}");
    let (contents, _) = gio::File::for_uri(&uri).load_contents_future().await.ok()?;
    let resp: Value = serde_json::from_slice(&contents).ok()?;
    let live_status = resp["data"]["live_status"].as_u64()?;
    Some(live_status == 1)
}

async fn resolve_real_room_id(
    client: &reqwest::Client,
    room_id: u64,
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    let resp: Value = client
        .get(format!(
            "https://api.live.bilibili.com/room/v1/Room/room_init?id={room_id}"
        ))
        .send()
        .await?
        .json()
        .await?;
    resp["data"]["room_id"]
        .as_u64()
        .ok_or_else(|| format!("missing room_id in room_init response: {resp}").into())
}

const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

fn wbi_mixin_key(orig: &[u8]) -> String {
    MIXIN_KEY_ENC_TAB
        .iter()
        .take(32)
        .map(|&i| orig[i] as char)
        .collect()
}

fn wbi_url_encode(s: &str) -> String {
    s.chars()
        .filter(|c| !"!'()*".contains(*c))
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.~".contains(c) {
                c.to_string()
            } else {
                c.encode_utf8(&mut [0; 4])
                    .bytes()
                    .map(|b| format!("%{b:02X}"))
                    .collect()
            }
        })
        .collect()
}

async fn fetch_wbi_keys(
    client: &reqwest::Client,
) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    let resp: Value = client
        .get("https://api.bilibili.com/x/web-interface/nav")
        .send()
        .await?
        .json()
        .await?;
    let take_filename = |url: &str| -> Option<String> {
        let (_, name) = url.rsplit_once('/')?;
        let (stem, _) = name.rsplit_once('.')?;
        Some(stem.to_string())
    };
    let img_key = resp["data"]["wbi_img"]["img_url"]
        .as_str()
        .and_then(take_filename)
        .ok_or_else(|| format!("missing wbi img_url in nav response: {resp}"))?;
    let sub_key = resp["data"]["wbi_img"]["sub_url"]
        .as_str()
        .and_then(take_filename)
        .ok_or_else(|| format!("missing wbi sub_url in nav response: {resp}"))?;
    Ok((img_key, sub_key))
}

fn wbi_sign(mut params: Vec<(&str, String)>, img_key: &str, sub_key: &str) -> String {
    let mixin_key = wbi_mixin_key(format!("{img_key}{sub_key}").as_bytes());
    let wts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    params.push(("wts", wts.to_string()));
    params.sort_by(|a, b| a.0.cmp(b.0));

    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", wbi_url_encode(k), wbi_url_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let w_rid = format!("{:x}", md5::compute(query.clone() + &mixin_key));
    format!("{query}&w_rid={w_rid}")
}

async fn fetch_buvid3(client: &reqwest::Client) -> String {
    let resp: Result<Value, _> = async {
        Ok::<_, reqwest::Error>(
            client
                .get("https://api.bilibili.com/x/frontend/finger/spi")
                .send()
                .await?
                .json()
                .await?,
        )
    }
    .await;
    resp.ok()
        .and_then(|v| v["data"]["b_3"].as_str().map(str::to_string))
        .unwrap_or_default()
}

async fn fetch_room_danmu_info(
    room_id: u64,
) -> Result<(u64, String, String, Vec<String>), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();

    let real_room_id = resolve_real_room_id(&client, room_id).await?;
    let buvid3 = fetch_buvid3(&client).await;
    let (img_key, sub_key) = fetch_wbi_keys(&client).await?;

    let query = wbi_sign(
        vec![("id", real_room_id.to_string()), ("type", "0".to_string())],
        &img_key,
        &sub_key,
    );

    let resp: Value = client
        .get(format!(
            "https://api.live.bilibili.com/xlive/web-room/v1/index/getDanmuInfo?{query}"
        ))
        .header(
            "Referer",
            format!("https://live.bilibili.com/{real_room_id}"),
        )
        .send()
        .await?
        .json()
        .await?;

    let token = resp["data"]["token"]
        .as_str()
        .ok_or_else(|| format!("missing token in getDanmuInfo response: {resp}"))?
        .to_string();
    let servers = resp["data"]["host_list"]
        .as_array()
        .ok_or_else(|| format!("missing host_list in getDanmuInfo response: {resp}"))?
        .iter()
        .filter_map(|h| {
            let host = h["host"].as_str()?;
            let wss_port = h["wss_port"].as_u64()?;
            Some(format!("wss://{host}:{wss_port}/sub"))
        })
        .collect();

    Ok((real_room_id, buvid3, token, servers))
}

fn build_room_enter_packet(real_room_id: u64, buvid3: &str, token: &str) -> Packet {
    let body = json!({
        "roomid": real_room_id,
        "uid": 0,
        "protover": 3,
        "platform": "web",
        "type": 2,
        "buvid": buvid3,
        "key": token,
    });
    Packet::new(
        Operation::RoomEnter,
        Protocol::Json,
        serde_json::to_vec(&body).unwrap_or_default(),
    )
}

fn decode_frame(buf: &[u8], out: &mut Vec<(u32, Vec<u8>)>) {
    let mut rest = buf;
    while rest.len() >= 16 {
        let packet_length = u32::from_be_bytes(rest[0..4].try_into().unwrap()) as usize;
        if packet_length < 16 || packet_length > rest.len() {
            break;
        }
        let protocol_version = u16::from_be_bytes(rest[6..8].try_into().unwrap());
        let op = u32::from_be_bytes(rest[8..12].try_into().unwrap());
        let data = &rest[16..packet_length];

        match protocol_version {
            2 => {
                let mut decompressed = Vec::new();
                if ZlibDecoder::new(data)
                    .read_to_end(&mut decompressed)
                    .is_ok()
                {
                    decode_frame(&decompressed, out);
                }
            }
            3 => {
                let mut decompressed = Vec::new();
                if brotli::Decompressor::new(data, 4096)
                    .read_to_end(&mut decompressed)
                    .is_ok()
                {
                    decode_frame(&decompressed, out);
                }
            }
            _ => out.push((op, data.to_vec())),
        }

        rest = &rest[packet_length..];
    }
}

async fn connect_once(
    real_room_id: u64,
    buvid3: &str,
    token: &str,
    server: &str,
    sender: &Sender<LiveDanmaku>,
    stop: &AtomicBool,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let (mut ws, _) = connect_async(server).await?;

    let enter = build_room_enter_packet(real_room_id, buvid3, token);
    ws.send(Message::binary(enter.encode())).await?;

    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));

    while stop.load(Ordering::Relaxed) {
        tokio::select! {
            _ = heartbeat.tick() => {
                let hb = Packet::new(Operation::HeartBeat, Protocol::Json, Vec::<u8>::new());
                ws.send(Message::binary(hb.encode())).await?;
            }
            msg = ws.next() => {
                let Some(msg) = msg else { return Ok(false) };
                let Message::Binary(data) = msg? else { continue };

                let mut packets = Vec::new();
                decode_frame(&data, &mut packets);
                for (op, payload) in packets {
                    if op != Operation::Notification as u32 {
                        continue;
                    }
                    let Ok(json) = serde_json::from_slice::<Value>(&payload) else { continue };
                    if json["cmd"].as_str() != Some("DANMU_MSG") {
                        continue;
                    }
                    let text = json["info"][1].as_str().unwrap_or("").to_string();
                    if text.is_empty() {
                        continue;
                    }
                    let color = json["info"][0][3].as_u64().unwrap_or(0xFF_FFFF) as u32;
                    if sender.send(LiveDanmaku { text, color }).is_err() {
                        return Ok(true);
                    }
                }
            }
        }
    }

    Ok(true)
}

async fn pace_danmaku(
    rx: flume::Receiver<LiveDanmaku>,
    sender: Sender<LiveDanmaku>,
    stop: Arc<AtomicBool>,
) {
    while stop.load(Ordering::Relaxed) {
        let Ok(dm) = rx.recv_async().await else { break };
        if sender.send(dm).is_err() {
            break;
        }

        let queued = rx.len() as u64 + 1;
        let interval_ms = 2000u64 / queued;
        if (50..=500).contains(&interval_ms) {
            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
        } else if interval_ms > 500 {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

pub fn spawn_bilibili_live_danmaku(
    room_id: u64,
    sender: Sender<LiveDanmaku>,
    stop: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let (real_room_id, buvid3, token, servers) =
                    match fetch_room_danmu_info(room_id).await {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::error!(
                                "bilibili live config fetch failed for room {room_id}: {e}"
                            );
                            return;
                        }
                    };

                let (raw_tx, raw_rx) = flume::unbounded::<LiveDanmaku>();
                let pacer_stop = Arc::clone(&stop);

                let receive = async move {
                    let mut backoff = Duration::from_millis(500);
                    let mut server_idx = 0;
                    while stop.load(Ordering::Relaxed) {
                        let server = &servers[server_idx % servers.len()];
                        server_idx += 1;

                        match connect_once(real_room_id, &buvid3, &token, server, &raw_tx, &stop)
                            .await
                        {
                            Ok(true) => break,
                            Ok(false) => {}
                            Err(e) => {
                                tracing::warn!(
                                    "bilibili live stream error for room {room_id}: {e}"
                                );
                            }
                        }

                        if !stop.load(Ordering::Relaxed) {
                            break;
                        }
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(30));
                    }
                };

                tokio::join!(receive, pace_danmaku(raw_rx, sender, pacer_stop));

                tracing::info!("bilibili live danmaku stopped for room {room_id}");
            });
    });
}
