// BoxQu für Android: Seiten der App in Compose; Bausteine (Bubble, Suche, Raster, Player, Theme) aus qu-core
// (`dev.roqu.qu`, android/src in qu-core), Daten aus Rust (boxqu-core über boxqu-ffi, `uniffi.boxqu_ffi`).
// Aussehen und Bedienung wie die Desktop-App; alles per Fernbedienung erreichbar (Android TV).
package dev.roqu.boxqu

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.*
import androidx.compose.ui.unit.dp
import androidx.media3.common.MimeTypes
import dev.roqu.qu.*
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import uniffi.boxqu_ffi.*
import java.io.File

/** Wie `default_quality` in qu.toml: höchstens 1080p, wenn es die Stufe gibt. */
const val QUALITY = 1080u

/** So viele Titel je Startseiten-Bereich holen (wie `SOURCE_FETCH` am Desktop). */
const val SECTION_FETCH = 30u

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        init(filesDir.path, cacheDir.path)
        enableEdgeToEdge()
        setContent { QuTheme { App() } }
    }
}

sealed interface Page {
    data object Home : Page
    data class Results(val query: String) : Page
    /** `follow`: zur bevorzugten Sprachfassung springen (nicht nach Tab-Klick). */
    data class Show(val item: Title, val follow: Boolean = true) : Page
    /** `episodes` null = beim Öffnen laden (z. B. aus „Continue watching“). */
    data class Watch(val item: Title, val number: UInt, val episodes: List<Episode>?) : Page
}

fun message(e: Exception) = (e as? BoxException.Failed)?.msg ?: e.message ?: "Something went wrong"

@Composable
fun App() {
    val nav = remember { Nav<Page>(Page.Home) }
    QuApp(
        nav,
        placeholder = "Search movies and series",
        searches = ::searches,
        onSearch = { addSearch(it); nav.open(Page.Results(it)) },
        bubble = { it !is Page.Watch },
    ) { page ->
        when (page) {
            Page.Home -> HomePage(
                open = { nav.open(Page.Show(it)) },
                resume = { nav.open(Page.Watch(it.item, it.episode, null)) },
            )
            is Page.Results -> ResultsPage(page.query) { nav.open(Page.Show(it)) }
            is Page.Show -> ShowPage(page, nav)
            is Page.Watch -> WatchPage(page, nav)
        }
    }
}

@Composable
fun TitleCard(t: Title, label: String? = t.kind, fraction: Double? = null, open: () -> Unit) =
    PosterCard(t.title, t.poster, label = label, fraction = fraction, onClick = open)

@Composable
fun HomePage(open: (Title) -> Unit, resume: (Resume) -> Unit) {
    val going = remember { continueWatching(12u) }
    val saved = remember { watchlist() }
    val parts = remember { sections() }
    // Je Bereich sofort die gemerkten Titel, dann frisch aus dem Netz.
    val shelves = remember { parts.map { mutableStateOf(savedTitles(it.url)) } }
    var error by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) {
        parts.forEachIndexed { i, s ->
            try { shelves[i].value = titles(s, SECTION_FETCH) } catch (e: Exception) { error = message(e) }
        }
    }
    Grid {
        var first = true
        fun top() = (if (first) 0 else 20).also { first = false }
        if (going.isNotEmpty()) {
            header("Continue watching", top().dp)
            items(going.size, key = { "c" + going[it].item.id }) { i ->
                val r = going[i]
                // Film (total 1): keine Episodennummer.
                TitleCard(r.item, if (r.total > 1u) "Episode ${r.episode}" else r.item.kind, r.fraction) { resume(r) }
            }
        }
        if (saved.isNotEmpty()) {
            header("Watchlist", top().dp)
            items(saved.size, key = { "w" + saved[it].id }) { TitleCard(saved[it]) { open(saved[it]) } }
        }
        val ids = saved.map { it.id }.toSet()
        parts.forEachIndexed { i, s ->
            header(s.name, top().dp)
            val list = shelves[i].value.filter { it.id !in ids }
            if (list.isEmpty() && error == null) ghosts(12)
            items(list.size, key = { "s$i" + list[it].id }) { TitleCard(list[it]) { open(list[it]) } }
        }
        error?.let { note(it, Red) }
    }
}

@Composable
fun ResultsPage(query: String, open: (Title) -> Unit) {
    var found by remember { mutableStateOf<List<Title>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(query) {
        try { found = search(query) } catch (e: Exception) { error = message(e) }
    }
    Grid {
        header(query, 0.dp)
        error?.let { note(it, Red) }
        val list = found
        when {
            list == null && error == null -> ghosts(12)
            list != null && list.isEmpty() -> note("Nothing found")
            list != null -> items(list.size, key = { list[it].id }) { TitleCard(list[it]) { open(list[it]) } }
        }
    }
}

@Composable
fun ShowPage(page: Page.Show, nav: Nav<Page>) {
    var item by remember { mutableStateOf(page.item) }
    var info by remember { mutableStateOf<Details?>(null) }
    var list by remember { mutableStateOf<List<Episode>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var listed by remember { mutableStateOf(onWatchlist(page.item.id)) }
    LaunchedEffect(page.item.id) {
        try {
            coroutineScope {
                val d = async { details(page.item.id) }
                val e = async { episodes(page.item.id) }
                val det = d.await()
                // Wie am Desktop: zur bevorzugten Sprachfassung springen (eigener Titel mit eigener ID).
                val pref = if (page.follow) preferred(det.languages) else null
                if (pref != null && pref.id != page.item.id) {
                    nav.replace(Page.Show(page.item.copy(id = pref.id), follow = false))
                    return@coroutineScope
                }
                info = det.also { item = it.item }
                list = e.await()
            }
        } catch (e: Exception) { error = message(e) }
    }
    Grid(minWidth = 170.dp) {
        wide {
            DetailHeader(
                item.title,
                item.poster,
                meta = info?.let { (it.info + it.genres).joinToString(" · ") },
                description = info?.description,
            ) {
                val langs = info?.languages.orEmpty()
                if (langs.size > 1) {
                    for (l in langs) {
                        TabButton(l.name, l.id == page.item.id) {
                            if (l.id != page.item.id) {
                                setLanguage(l)
                                nav.replace(Page.Show(page.item.copy(id = l.id), follow = false))
                            }
                        }
                    }
                }
                IconButton(
                    if (listed) QuIcons.watchlisted else QuIcons.watchlist,
                    if (listed) "Remove from watchlist" else "Add to watchlist",
                    tint = if (listed) Accent else Fg,
                ) { listed = toggleWatchlist(item) }
            }
        }
        error?.let { note(it, Red) }
        val eps = list
        if (eps == null && error == null) ghostTiles(6)
        if (eps != null && eps.isEmpty()) note("Nothing to play yet")
        if (eps != null) {
            items(eps.size, key = { eps[it].id }) { i ->
                val ep = eps[i]
                val seen = remember(ep.id) { watched(item.id, ep.number) }
                // Film = eine Kachel „Play“.
                val (title, sub) = if (eps.size == 1) "Play" to (item.duration ?: "") else "Episode ${ep.number}" to ep.title
                Tile(title, sub, seen?.fraction, seen?.finished == true) { nav.open(Page.Watch(item, ep.number, eps)) }
            }
        }
    }
}

@Composable
fun WatchPage(page: Page.Watch, nav: Nav<Page>) {
    var list by remember { mutableStateOf(page.episodes) }
    val following = list?.firstOrNull { it.number > page.number }
    VideoPlayer(
        load = {
            val eps = list ?: episodes(page.item.id).also { list = it }
            val s = stream(eps.first { it.number == page.number }.id)
            val v = s.variants.firstOrNull { it.height in 1u..QUALITY } ?: s.variants.first()
            Video(
                url = v.url,
                // Lokaler DASH-Zwischenserver bzw. Manifest: …/index.mpd; sonst direkte Datei.
                mime = if (v.url.substringBefore('?').endsWith(".mpd")) MimeTypes.APPLICATION_MPD else null,
                headers = s.headers.associate { it.name to it.value },
                subtitles = s.subtitles.map { Subtitle(it, File(it).nameWithoutExtension, MimeTypes.APPLICATION_SUBRIP) },
                start = resumeAt(page.item.id, page.number),
                speed = speed(page.item.id).toFloat(),
            )
        },
        message = ::message,
        save = { pos, dur -> record(page.item, page.number, (list?.size ?: 1).toUInt(), pos, dur) },
        next = following?.let { n -> { nav.replace(page.copy(number = n.number, episodes = list)) } },
    )
}
