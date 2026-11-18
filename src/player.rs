//! Player-Anbindung: der Player aus qu-core mit BoxQu-Daten – Fortschritt pro Episode, Tempo pro Titel,
//! Stream mit HTTP-Headern und Untertitel-Dateien.

use crate::progress::{self, Meta};
use crate::source::Stream;
use qu_core::player::{Hooks, Media, header_opts, quoted};
use qu_core::segments::Segment;
use std::future::Future;
use std::pin::Pin;

/// Abgespielt wird eine Episode (Film = Episode 1), dazu die MAL-ID für AniSkip (bei MovieBox nie gesetzt).
pub type Item = (Meta, Option<String>);
pub type Player = qu_core::player::Player<Item>;

struct BoxHooks;

impl Hooks<Item> for BoxHooks {
    fn init(&self, i: &libmpv2::MpvInitializer) -> libmpv2::Result<()> {
        i.set_property("user-agent", crate::source::USER_AGENT)
    }

    fn save(&self, (m, _): &Item, pos: f64, dur: f64) {
        progress::record(m, pos, dur);
    }

    fn segments(&self, (m, mal): &Item) -> Option<Pin<Box<dyn Future<Output = Vec<Segment>> + Send>>> {
        Some(Box::pin(crate::aniskip::segments(mal.clone()?, m.episode)))
    }

    fn speed_changed(&self, (m, _): &Item, speed: f64) {
        crate::lists::set_anime_speed(&m.anime, speed);
    }
}

pub fn new() -> Player {
    Player::new(BoxHooks)
}

/// Episode abspielen: fortsetzen, wo man aufgehört hat; Tempo, das zuletzt für diesen Titel galt.
pub fn media(meta: Meta, stream: Stream) -> Media<Item> {
    let mut opts = header_opts(&stream.headers);
    if !stream.subtitles.is_empty() {
        // Lokale Dateien (`source::stream` lädt sie vorher): `sub-files` ist eine Pfadliste, Trenner wie bei PATH
        // („:“, unter Windows „;“, dort steckt „:“ ja in „C:\“).
        let sep = if cfg!(windows) { ";" } else { ":" };
        opts.push(format!("sub-files={}", quoted(&stream.subtitles.join(sep))));
    }
    Media {
        start: progress::resume_at(&meta.anime, meta.episode),
        speed: crate::lists::anime_speed(&meta.anime),
        item: Some((meta, stream.mal_id)),
        variants: stream.variants,
        opts,
        live: false,
        poster: false,
    }
}
