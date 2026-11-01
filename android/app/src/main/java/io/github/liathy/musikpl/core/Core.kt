package io.github.liathy.musikpl.core

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import io.github.liathy.musikpl.playback.ExoAudio
import java.util.concurrent.Executors
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import uniffi.musikpl_mobile.AndroidPlatform
import uniffi.musikpl_mobile.ArtistState
import uniffi.musikpl_mobile.HomeState
import uniffi.musikpl_mobile.MobileEngine
import uniffi.musikpl_mobile.PlayerState
import uniffi.musikpl_mobile.PlaylistDetail
import uniffi.musikpl_mobile.PlaylistSummary
import uniffi.musikpl_mobile.ProfileState
import uniffi.musikpl_mobile.QueueState
import uniffi.musikpl_mobile.SearchState
import uniffi.musikpl_mobile.SettingsState
import uniffi.musikpl_mobile.Song

/**
 * The Rust engine, hosted for the whole app.
 *
 * Every call into the engine runs on one thread, in order. A loop ticks it
 * as often as it asks (and right away when background work wakes it), and
 * after each tick publishes snapshots for the screens. Callbacks from the
 * engine (the player, the extractor) never call back into it.
 */
object Core {
    lateinit var engine: MobileEngine
        private set
    lateinit var audio: ExoAudio
        private set
    private lateinit var app: Context

    private val thread = Executors.newSingleThreadExecutor { Thread(it, "musikpl-engine") }
    private val scope = CoroutineScope(SupervisorJob() + thread.asCoroutineDispatcher())
    private val wakeUp = Channel<Unit>(Channel.CONFLATED)
    private val main = Handler(Looper.getMainLooper())

    private val _player = MutableStateFlow<PlayerState?>(null)
    private val _home = MutableStateFlow<HomeState?>(null)
    private val _search = MutableStateFlow<SearchState?>(null)
    private val _queue = MutableStateFlow<QueueState?>(null)
    private val _playlists = MutableStateFlow<List<PlaylistSummary>>(emptyList())
    private val _playlist = MutableStateFlow<PlaylistDetail?>(null)
    private val _liked = MutableStateFlow<List<Song>>(emptyList())
    private val _artist = MutableStateFlow<ArtistState?>(null)
    private val _profile = MutableStateFlow<ProfileState?>(null)
    private val _settings = MutableStateFlow<SettingsState?>(null)

    val player: StateFlow<PlayerState?> = _player
    val home: StateFlow<HomeState?> = _home
    val search: StateFlow<SearchState?> = _search
    val queue: StateFlow<QueueState?> = _queue
    val playlists: StateFlow<List<PlaylistSummary>> = _playlists
    val playlist: StateFlow<PlaylistDetail?> = _playlist
    val liked: StateFlow<List<Song>> = _liked
    val artist: StateFlow<ArtistState?> = _artist
    val profile: StateFlow<ProfileState?> = _profile
    val settings: StateFlow<SettingsState?> = _settings

    /** The screens are showing: keep their snapshots fresh. */
    @Volatile
    var visible = false
        set(value) {
            field = value
            if (value) wake()
        }

    /** Text the engine asked to copy (a friend code). */
    val clipboard = MutableStateFlow<String?>(null)

    fun init(context: Context) {
        app = context.applicationContext
        audio = ExoAudio(app)
        val extractor = YouTubeExtractor(app)
        engine = MobileEngine(
            dataDir = app.filesDir.resolve("MusiKPL").absolutePath,
            player = audio,
            extractor = extractor,
            app = Platform(),
        )
        scope.launch {
            engine.start()
            loop()
        }
    }

    /** Ask the engine to do something (on its thread), then refresh. */
    fun act(block: MobileEngine.() -> Unit) {
        scope.launch {
            engine.block()
            engine.tick()
            refresh()
        }
    }

    /** Background work has news: tick now. */
    fun wake() {
        wakeUp.trySend(Unit)
    }

    /** Save everything now (the app may be killed). */
    fun save() {
        scope.launch { engine.shutDown() }
    }

    private suspend fun loop() {
        while (true) {
            val next = runCatching { engine.tick() }.getOrDefault(1000u).toLong()
            refresh()
            // While the screens show, refresh often enough for the seek bar.
            val wait = if (visible) minOf(next, 250L) else next
            withTimeoutOrNull(wait) { wakeUp.receive() }
        }
    }

    private fun refresh() {
        val state = engine.playerState()
        _player.value = state
        audio.showing(state.song)
        engine.takeClipboard()?.let { clipboard.value = it }
        if (!visible) return
        _home.value = engine.home()
        _search.value = engine.searchState()
        _queue.value = engine.queue()
        _playlists.value = engine.playlists()
        _playlist.value = engine.playlist()
        _liked.value = engine.likedSongs()
        _artist.value = engine.artist()
        _profile.value = engine.profile()
        _settings.value = engine.settings()
    }

    private class Platform : AndroidPlatform {
        override fun openUrl(url: String): Boolean {
            main.post {
                runCatching {
                    app.startActivity(
                        Intent(Intent.ACTION_VIEW, Uri.parse(url))
                            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                    )
                }
            }
            return true
        }

        override fun wake() = Core.wake()
    }
}
