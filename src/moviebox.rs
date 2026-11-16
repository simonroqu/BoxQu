//! MovieBox-API (App-Schnittstelle von aoneroom): signierte Anfragen, Besucher-Session, Host-Pool.
//! Übernommen aus MovieBox-Tui (MIT/Apache-2.0, `src/providers/moviebox/{client,crypto,session}.rs`),
//! auf das gekürzt, was BoxQu braucht. Liefert rohes JSON (`data`-Teil); Auswertung in `source.rs`.

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use md5::{Digest, Md5};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const HOSTS: &[&str] = &[
    "https://api6.aoneroom.com",
    "https://api5.aoneroom.com",
    "https://api4.aoneroom.com",
    "https://api4sg.aoneroom.com",
    "https://api3.aoneroom.com",
    "https://api6sg.aoneroom.com",
    "https://api.inmoviebox.com",
];
/// Bei diesen Antworten den nächsten Host versuchen.
const RETRY_STATUS: &[u16] = &[403, 406, 407, 429, 500, 502, 503, 504];
/// Schlüssel der App für die `x-tr-signature` (HMAC-MD5).
const SECRET: &[u8] = b"\xef\xa8\x91\x97\x4e\xec\xd3\x14\x8d\xf6\x3a\xa6\x11\x60\x2d\xef\xd1\x01\x25\x9b\xa5\x21\x02\x2c\x57\xae\x05\x66\xbd\x8e";
const SIGNATURE_BODY_MAX: usize = 102_400;
const STOCK_CERT_SHA256: &str = "03d4b741632d136bc23e2de4c6adcdb7712f1980b619c183221801730bbd0410";
/// Referer, den die DASH-Streams (play-info) verlangen.
pub const STREAM_REFERER: &str = "https://sportslive.wine";

/// Gerät, als das wir auftreten (einmal pro Programmstart ausgewürfelt, wie die TUI).
struct Device {
    user_agent: String,
    client_info: String,
    ip: String,
}

static DEVICE: LazyLock<Device> = LazyLock::new(|| {
    let android = pick(&[("9", "PQ3A.190605.03081104"), ("10", "QP1A.191005.007.A3"), ("11", "RP1A.200720.011"), ("12", "S1B.220414.015"), ("13", "TQ2A.230405.003")]);
    let model = pick(&["23078RKD5C", "2201117TY", "2201117TG", "22101316G", "21121210G", "M2012K11AG", "M2007J20CG"]);
    let code = pick(&[50020117, 50020118, 50020119, 50020120, 50020121]);
    let net = pick(&["NETWORK_WIFI", "NETWORK_MOBILE"]);
    let tz = pick(&["Asia/Kolkata", "Asia/Shanghai", "Asia/Tokyo", "America/New_York", "Europe/London"]);
    let gaid = format!("{}-{}-{}-{}-{}", hex(8), hex(4), hex(4), hex(4), hex(12));
    let user_agent = format!("com.community.oneroom/{code} (Linux; U; Android {}; en_US; {model}; Build/{}; Cronet/135.0.7012.3)", android.0, android.1);
    let client_info = format!(
        r#"{{"package_name":"com.community.oneroom","version_name":"4.0.01.0813.03","version_code":{code},"os":"android","os_version":"{}","install_ch":"ps","device_id":"{}","install_store":"ps","gaid":"{gaid}","brand":"Redmi","model":"{model}","system_language":"en","net":"{net}","region":"US","timezone":"{tz}","sp_code":"40401","X-Play-Mode":"2"}}"#,
        android.0,
        hex(32)
    );
    let prefix = pick(&["103.241", "49.36", "117.195", "106.198", "122.162", "157.32", "182.70", "103.58", "27.60", "59.90"]);
    let ip = format!("{prefix}.{}.{}", 1 + rand(253), 1 + rand(253));
    Device { user_agent, client_info, ip }
});

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .connect_timeout(Duration::from_secs(3))
        .build()
        .expect("HTTP-Client")
});
/// Host, der zuletzt geantwortet hat (dort fängt die nächste Anfrage an).
static ACTIVE_HOST: AtomicUsize = AtomicUsize::new(0);
static TOKEN: Mutex<Option<String>> = Mutex::new(None);
/// Nur eine Anmeldung gleichzeitig.
static LOGIN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn user_agent() -> &'static str {
    &DEVICE.user_agent
}

// ---------- Zufall (ohne eigene Abhängigkeit) ----------

fn rand(n: u32) -> u32 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos());
    (h.finish() % n as u64) as u32
}

fn pick<T: Copy>(list: &[T]) -> T {
    list[rand(list.len() as u32) as usize]
}

fn hex(len: usize) -> String {
    (0..len).map(|_| format!("{:x}", rand(16))).collect()
}

fn md5_hex(b: &[u8]) -> String {
    Md5::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

// ---------- Signatur ----------

/// `x-client-build`: Zeitstempel + Zufall, gehasht mit dem Zertifikat der Original-App.
fn build_tag(ts_ms: u64, rand_part: u32, cert: &str) -> String {
    let stamp = format!("{}{:03}000{}", ts_ms / 1000, ts_ms % 1000, rand_part);
    format!("{stamp}.{}", md5_hex(format!("{cert}oneroom{stamp}").as_bytes()))
}

/// Pfad + nach Schlüssel sortierte Query (so signiert die App).
fn canonical_url(url: &str) -> String {
    let Ok(u) = reqwest::Url::parse(url) else { return url.to_string() };
    let mut q: Vec<(String, String)> = u.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    // Stabil sortieren: gleiche Schlüssel behalten ihre Reihenfolge (wie BTreeMap<_, Vec<_>> in der TUI).
    q.sort_by(|a, b| a.0.cmp(&b.0));
    let q: Vec<String> = q.into_iter().map(|(k, v)| format!("{k}={v}")).collect();
    if q.is_empty() { u.path().to_string() } else { format!("{}?{}", u.path(), q.join("&")) }
}

fn signature(method: &str, url: &str, body: Option<&str>, ts: u64) -> String {
    let (hash, len) = match body {
        Some(b) => (md5_hex(&b.as_bytes()[..b.len().min(SIGNATURE_BODY_MAX)]), b.len().to_string()),
        None => (String::new(), String::new()),
    };
    let canonical = format!("{method}\napplication/json\napplication/json\n{len}\n{ts}\n{hash}\n{}", canonical_url(url));
    let mut mac = Hmac::<Md5>::new_from_slice(SECRET).expect("HMAC-Schlüssel");
    mac.update(canonical.as_bytes());
    format!("{ts}|2|{}", base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes()))
}

fn signed_headers(method: &str, url: &str, body: Option<&str>, token: Option<&str>) -> Vec<(&'static str, String)> {
    let ts = now_ms();
    let tag = build_tag(ts, rand(1000), STOCK_CERT_SHA256);
    let reversed: String = ts.to_string().chars().rev().collect();
    let info = match serde_json::from_str::<Value>(&DEVICE.client_info) {
        Ok(mut v) => {
            v["X-Client-Build"] = tag.clone().into();
            v.to_string()
        }
        Err(_) => DEVICE.client_info.clone(),
    };
    let mut h = vec![
        ("user-agent", DEVICE.user_agent.clone()),
        ("accept", "application/json".into()),
        ("content-type", "application/json".into()),
        ("connection", "keep-alive".into()),
        ("x-client-token", format!("{ts},{}", md5_hex(reversed.as_bytes()))),
        ("x-tr-signature", signature(method, url, body, ts)),
        ("x-client-build", tag),
        ("x-client-info", info),
        ("x-client-status", "0".into()),
        ("x-forwarded-for", DEVICE.ip.clone()),
    ];
    if let Some(t) = token {
        h.push(("authorization", format!("Bearer {t}")));
    }
    h
}

// ---------- Session (Besucher-Login, Token auf der Platte) ----------

fn session_file() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache/boxqu/moviebox-session")
}

/// Ablaufzeit aus dem JWT (`exp`); ohne: 7 Tage ab jetzt.
fn expiry(token: &str) -> u64 {
    let claims = token.split('.').nth(1).and_then(|p| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(p.trim_end_matches('=')).ok());
    let exp = claims.and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v["exp"].as_u64().or_else(|| v["exp"].as_str()?.parse().ok()));
    exp.unwrap_or(now_ms() / 1000 + 7 * 24 * 3600)
}

fn valid(token: &str) -> bool {
    !token.trim().is_empty() && now_ms() / 1000 + 60 < expiry(token)
}

fn remember(token: &str) {
    *TOKEN.lock().unwrap() = Some(token.to_string());
    let p = session_file();
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let _ = std::fs::write(p, token);
}

fn forget() {
    *TOKEN.lock().unwrap() = None;
    let _ = std::fs::remove_file(session_file());
}

fn cached_token() -> Option<String> {
    if let Some(t) = TOKEN.lock().unwrap().clone().filter(|t| valid(t)) {
        return Some(t);
    }
    let t = std::fs::read_to_string(session_file()).ok().map(|s| s.trim().to_string()).filter(|t| valid(t))?;
    *TOKEN.lock().unwrap() = Some(t.clone());
    Some(t)
}

async fn token() -> Result<String, String> {
    if let Some(t) = cached_token() {
        return Ok(t);
    }
    let _guard = LOGIN.lock().await;
    if let Some(t) = cached_token() {
        return Ok(t);
    }
    let v = request_hosts("POST", "/wefeed-mobile-bff/user-api/visitor-login", Some("{}"), None).await?;
    let t = v["token"].as_str().filter(|t| !t.trim().is_empty()).ok_or("MovieBox login failed")?.to_string();
    remember(&t);
    Ok(t)
}

// ---------- Anfragen ----------

pub async fn get(path: &str) -> Result<Value, String> {
    request("GET", path, None).await
}

pub async fn post(path: &str, body: &Value) -> Result<Value, String> {
    request("POST", path, Some(&body.to_string())).await
}

/// Mit Token anfragen; wird es abgelehnt (oder antwortet kein Host), einmal neu anmelden.
async fn request(method: &str, path: &str, body: Option<&str>) -> Result<Value, String> {
    let t = token().await?;
    match request_hosts(method, path, body, Some(&t)).await {
        Err(e) if e.starts_with(AUTH_ERR) || e == EXHAUSTED => {
            forget();
            let t = token().await?;
            request_hosts(method, path, body, Some(&t)).await
        }
        r => r,
    }
}

const AUTH_ERR: &str = "MovieBox refused the request";
const EXHAUSTED: &str = "MovieBox is not reachable";

/// Hosts der Reihe nach probieren, beim zuletzt funktionierenden anfangen.
async fn request_hosts(method: &str, path: &str, body: Option<&str>, token: Option<&str>) -> Result<Value, String> {
    let start = ACTIVE_HOST.load(Ordering::Relaxed);
    let mut wait = 50;
    for i in 0..HOSTS.len() {
        if i > 0 {
            tokio::time::sleep(Duration::from_millis(wait)).await;
            wait = 50;
        }
        let idx = (start + i) % HOSTS.len();
        let url = format!("{}{path}", HOSTS[idx]);
        let mut req = if method == "POST" { HTTP.post(&url) } else { HTTP.get(&url) };
        for (k, v) in signed_headers(method, &url, body, token) {
            req = req.header(k, v);
        }
        if let Some(b) = body {
            req = req.body(b.to_string());
        }
        let res = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("MovieBox-Host {idx}: {e}");
                ACTIVE_HOST.store((idx + 1) % HOSTS.len(), Ordering::Relaxed);
                continue;
            }
        };
        // Der Server tauscht das Token manchmal über den Header `x-user` aus.
        if let Some(t) = res.headers().get("x-user").and_then(|v| v.to_str().ok()).and_then(|s| serde_json::from_str::<Value>(s).ok()) {
            if let Some(t) = t["token"].as_str().filter(|t| !t.is_empty()) {
                remember(t);
            }
        }
        let status = res.status().as_u16();
        if RETRY_STATUS.contains(&status) {
            ACTIVE_HOST.store((idx + 1) % HOSTS.len(), Ordering::Relaxed);
            if status == 429 {
                let secs = res.headers().get("retry-after").and_then(|v| v.to_str().ok()?.parse::<u64>().ok());
                wait = secs.map_or(400, |s| (s * 1000).min(3000));
            }
            continue;
        }
        ACTIVE_HOST.store(idx, Ordering::Relaxed);
        if status == 401 {
            return Err(format!("{AUTH_ERR} (HTTP 401)"));
        }
        if !res.status().is_success() {
            return Err(format!("MovieBox error (HTTP {status})"));
        }
        match res.text().await.map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string())) {
            Ok(mut v) => return Ok(if v.get("data").is_some() { v["data"].take() } else { v }),
            Err(e) => eprintln!("MovieBox-Host {idx}: {e}"),
        }
    }
    Err(EXHAUSTED.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_matches_moviebox_tui() {
        // Testvektoren aus MovieBox-Tui (crypto.rs), Zertifikat = sha256("stock-certificate").
        let cert = "1d577a48483a9dff6d67c73f331a96ce35553acfb916a28a55533320126de01f";
        assert_eq!(build_tag(1700000000123, 45, cert), "170000000012300045.1a8f8b991f95799fba169a8a7f889ebc");
        assert_eq!(build_tag(1700000000007, 7, cert), "17000000000070007.d1c4e3525f7a8bf894ffdb6fcda377b2");
        assert_eq!(canonical_url("https://x/p?b=2&a=1&c=3"), "/p?a=1&b=2&c=3");
        let sig = signature("GET", "https://x/p?tab=1", None, 1700000000000);
        assert!(sig.starts_with("1700000000000|2|") && sig.len() > 20);
    }

    #[test]
    fn device_looks_like_the_app() {
        assert!(user_agent().starts_with("com.community.oneroom/500201"));
        let info: Value = serde_json::from_str(&DEVICE.client_info).unwrap();
        assert_eq!(info["version_name"], "4.0.01.0813.03");
        assert_eq!(DEVICE.ip.split('.').count(), 4);
    }

    #[test]
    fn token_expiry() {
        let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"exp":1893456000}"#);
        assert_eq!(expiry(&format!("h.{claims}.s")), 1893456000);
        assert!(!valid(&format!("h.{}.s", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"exp":100}"#))));
        assert!(!valid(""));
    }
}
