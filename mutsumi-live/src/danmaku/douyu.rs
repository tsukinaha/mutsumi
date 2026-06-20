use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use async_tungstenite::tungstenite::Message;
use flume::Sender;
use futures::{SinkExt, StreamExt};

use super::LiveDanmaku;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:146.0) Gecko/20100101 Firefox/146.0";

const HEARTBEAT: &[u8] =
    b"\x14\x00\x00\x00\x14\x00\x00\x00\xb1\x02\x00\x00\x74\x79\x70\x65\x40\x3d\x6d\x72\x6b\x6c\x2f\x00";

static COLORS: &[(&str, u32)] = &[
    ("1", 0xff0000),
    ("2", 0x1e87f0),
    ("3", 0x7ac84b),
    ("4", 0xff7f00),
    ("5", 0x9b39f4),
    ("6", 0xff69b4),
];

fn lookup_color(col: &str) -> u32 {
    COLORS
        .iter()
        .find(|(k, _)| *k == col)
        .map(|(_, v)| *v)
        .unwrap_or(0xffffff)
}

pub fn parse_douyu_room_id(url: &str) -> Option<String> {
    let url = url.trim();
    let path = url
        .strip_prefix("https://www.douyu.com/")
        .or_else(|| url.strip_prefix("http://www.douyu.com/"))
        .or_else(|| url.strip_prefix("https://douyu.com/"))
        .or_else(|| url.strip_prefix("http://douyu.com/"))?;
    let rid = path.split(['?', '#', '/']).next()?;
    if rid.is_empty() {
        None
    } else {
        tracing::info!("douyu: parsed room id {rid:?} from url {url:?}");
        Some(rid.to_string())
    }
}

/// Resolves a Douyu room alias (e.g. "6657") to the numeric room ID (e.g. "6979222").
/// Douyu room pages embed the real room ID in cover image URLs on douyucdn.cn.
/// Returns the input unchanged if already a numeric ID or on fetch failure.
async fn resolve_real_rid(client: &reqwest::Client, rid: &str) -> String {
    let html = match async {
        client
            .get(format!("https://www.douyu.com/{rid}"))
            .header("User-Agent", UA)
            .send()
            .await?
            .text()
            .await
    }
    .await
    {
        Ok(h) => h,
        Err(_) => return rid.to_string(),
    };
    // Cover images: douyucdn.cn/asrpic/{date}/{real_rid}_...
    html.split("douyucdn.cn/asrpic/")
        .nth(1)
        .and_then(|s| s.split('/').nth(1))
        .and_then(|s| s.split('_').next())
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        .map(|real| {
            if real != rid {
                tracing::info!("douyu: resolved alias {rid:?} -> {real:?}");
            }
            real.to_string()
        })
        .unwrap_or_else(|| rid.to_string())
}

pub async fn check_douyu_live_status(rid: &str) -> Option<bool> {
    let rid = rid.to_string();
    let (tx, rx) = flume::bounded(1);

    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let result: Option<bool> = async {
                    let client = reqwest::Client::new();
                    let rid = resolve_real_rid(&client, &rid).await;
                    let j = client
                        .get(format!("https://www.douyu.com/betard/{rid}"))
                        .header("User-Agent", UA)
                        .header("Referer", format!("https://www.douyu.com/{rid}"))
                        .send()
                        .await
                        .ok()?
                        .json::<serde_json::Value>()
                        .await
                        .ok()?;
                    let show_status = j["room"]["show_status"].as_i64()?;
                    let video_loop = j["room"]["videoLoop"].as_i64()?;
                    Some(show_status == 1 && video_loop == 0)
                }
                .await;
                let _ = tx.send(result);
            });
    });

    rx.recv_async().await.ok().flatten()
}

/// Returns `(stream_url, real_rid)`. The real_rid may differ from the input
/// when the input is a Douyu room alias (slug) rather than a numeric room ID.
pub async fn get_douyu_stream_url(
    rid: &str,
) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    const API2: &str = "https://www.douyu.com/swf_api/homeH5Enc?rids=";
    const API3: &str = "https://www.douyu.com/lapi/live/getH5Play/";

    let client = reqwest::Client::new();
    let real_rid = resolve_real_rid(&client, rid).await;
    let rid = real_rid.as_str();

    let resp = client
        .get(format!("{API2}{rid}"))
        .header("User-Agent", UA)
        .header("Referer", format!("https://www.douyu.com/{rid}"))
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    let js_enc = resp
        .pointer(&format!("/data/room{rid}"))
        .and_then(|x| x.as_str())
        .ok_or("missing room JS in homeH5Enc response")?
        .to_string();

    let crypto_js = include_str!("crypto-js.min.js");
    let did = uuid::Uuid::new_v4().simple().to_string();
    let tsec = format!("{}", chrono::Local::now().timestamp());

    let rt = rquickjs::Runtime::new()?;
    let ctx = rquickjs::Context::full(&rt)?;
    let enc_data = ctx.with(|ctx| -> rquickjs::Result<String> {
        ctx.eval::<(), _>(crypto_js)?;
        ctx.eval::<(), _>(js_enc.as_str())?;
        ctx.eval::<String, _>(format!("ub98484234('{rid}','{did}','{tsec}')"))
    })?;

    let mut params: Vec<(&str, &str)> = enc_data
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .collect();
    params.push(("cdn", ""));
    params.push(("iar", "0"));
    params.push(("ive", "0"));
    params.push(("rate", "0"));

    let resp = client
        .post(format!("{API3}{rid}"))
        .header("User-Agent", UA)
        .header("Referer", format!("https://www.douyu.com/{rid}"))
        .form(&params)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    let error_code = resp["error"].as_i64().unwrap_or(0);
    if error_code != 0 {
        let msg = resp["msg"].as_str().unwrap_or("unknown error");
        return Err(format!("douyu getH5Play error {error_code}: {msg}").into());
    }

    let rtmp_url = resp
        .pointer("/data/rtmp_url")
        .and_then(|x| x.as_str())
        .ok_or("missing rtmp_url")?;
    let rtmp_live = resp
        .pointer("/data/rtmp_live")
        .and_then(|x| x.as_str())
        .ok_or("missing rtmp_live")?;

    Ok((format!("{rtmp_url}/{rtmp_live}"), real_rid))
}

fn build_packet(payload: &str) -> Vec<u8> {
    let len = payload.len() as u32 + 9;
    let mut data = Vec::with_capacity(payload.len() + 13);
    data.extend_from_slice(&len.to_le_bytes());
    data.extend_from_slice(&len.to_le_bytes());
    data.extend_from_slice(b"\xb1\x02\x00\x00");
    data.extend_from_slice(payload.as_bytes());
    data.push(0x00);
    data
}

// Douyu binary frame: [4B frame_len][frame_len bytes: [4B msg_len][4B magic \xb1\x02\x00\x00][payload][1B \x00][1B \x02]]
// payload = msg_len - 10 bytes
fn decode_packets(data: &[u8]) -> Vec<LiveDanmaku> {
    let mut danmakus = Vec::new();
    let mut pos = 0;

    loop {
        if pos + 4 > data.len() {
            break;
        }
        let msg_len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        if msg_len < 10 || pos + msg_len > data.len() {
            break;
        }

        // skip inner len (4B) + magic (4B), read payload, skip trailing null+sep (2B)
        let payload = &data[pos + 8..pos + msg_len - 2];
        pos += msg_len;

        let msg = String::from_utf8_lossy(payload);
        // Douyu STT encoding: @= is key/value separator, / is field separator
        // @A and @S are escaped @ and /
        let msg = msg.replace("@=", r#"":""#).replace('/', r#"",""#);
        let msg = msg.replace("@A", "@").replace("@S", "/");
        let msg = format!(r#"{{"{}"}}"#, &msg);

        let j: serde_json::Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if j["type"].as_str() != Some("chatmsg") {
            continue;
        }
        let text = match j["txt"].as_str() {
            Some(t) => t.trim().to_string(),
            None => continue,
        };
        if text.is_empty() {
            continue;
        }
        let col = j["col"].as_str().unwrap_or("-1");
        danmakus.push(LiveDanmaku {
            text,
            color: lookup_color(col),
        });
    }

    danmakus
}

async fn connect_once(
    rid: &str,
    sender: &Sender<LiveDanmaku>,
    stop: &AtomicBool,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let (ws, _) =
        async_tungstenite::tokio::connect_async("wss://danmuproxy.douyu.com:8505").await?;
    let (mut ws_write, mut ws_read) = ws.split();

    ws_write
        .send(Message::Binary(build_packet(&format!(
            "type@=loginreq/roomid@={rid}/"
        ))))
        .await?;
    ws_write
        .send(Message::Binary(build_packet(&format!(
            "type@=joingroup/rid@={rid}/gid@=1/"
        ))))
        .await?;

    let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
    heartbeat.tick().await;

    while stop.load(Ordering::Relaxed) {
        tokio::select! {
            _ = heartbeat.tick() => {
                ws_write.send(Message::Binary(HEARTBEAT.to_vec())).await?;
            }
            msg = ws_read.next() => {
                let Some(msg) = msg else { return Ok(false) };
                for dm in decode_packets(&msg?.into_data()) {
                    if sender.send(dm).is_err() {
                        return Ok(true);
                    }
                }
            }
        }
    }

    Ok(true)
}

pub fn spawn_douyu_live_danmaku(rid: String, sender: Sender<LiveDanmaku>, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let mut backoff = Duration::from_millis(500);
                while stop.load(Ordering::Relaxed) {
                    match connect_once(&rid, &sender, &stop).await {
                        Ok(true) => break,
                        Ok(false) => {}
                        Err(e) => tracing::warn!("douyu danmaku error for {rid}: {e}"),
                    }
                    if !stop.load(Ordering::Relaxed) {
                        break;
                    }
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
                tracing::info!("douyu danmaku stopped for {rid}");
            });
    });
}
