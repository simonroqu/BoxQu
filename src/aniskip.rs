//! ani-skip: von der Community markierte Openings und Endings; der Player bietet dort „Skip intro“ an.
//! Fragt api.aniskip.com (MyAnimeList-ID + Episodennummer).

use std::sync::LazyLock;

pub use qu_core::segments::Segment;

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

/// Abschnitte zum Überspringen. Leer, wenn es keine gibt oder ani-skip nicht erreichbar ist.
pub async fn segments(mal_id: String, episode: u32) -> Vec<Segment> {
    let url = format!("https://api.aniskip.com/v2/skip-times/{mal_id}/{episode}?types[]=op&types[]=ed&episodeLength=0");
    let Ok(res) = HTTP.get(url).send().await else { return vec![] };
    let Ok(text) = res.text().await else { return vec![] };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    parse(&json)
}

fn parse(json: &serde_json::Value) -> Vec<Segment> {
    let mut out: Vec<Segment> = json["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let label = match r["skipType"].as_str()? {
                "op" => "intro",
                "ed" => "ending",
                _ => return None,
            };
            let start = r["interval"]["startTime"].as_f64()?;
            let end = r["interval"]["endTime"].as_f64()?;
            (end > start).then_some(Segment { start, end, label })
        })
        .collect();
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    out
}


#[cfg(test)]
mod tests {
    use super::*;
    use qu_core::segments::segment_at;

    #[test]
    fn parses_and_decides() {
        let json = serde_json::json!({"found": true, "results": [
            {"interval": {"startTime": 1417.1, "endTime": 1507.1}, "skipType": "ed"},
            {"interval": {"startTime": 3.2, "endTime": 93.2}, "skipType": "op"},
            {"interval": {"startTime": 5.0, "endTime": 9.0}, "skipType": "recap"}
        ]});
        let s = parse(&json);
        assert_eq!(s.len(), 2, "nur Opening und Ending");
        assert_eq!((s[0].start, s[0].label), (3.2, "intro"));
        assert_eq!(segment_at(&s, 10.0), Some(0));
        assert_eq!(segment_at(&s, 1420.0), Some(1));
        assert_eq!(segment_at(&s, 93.0), None, "kurz vor Ende nicht mehr anbieten");
        assert_eq!(segment_at(&s, 500.0), None);
        assert!(parse(&serde_json::json!({"found": false})).is_empty());
    }
}
