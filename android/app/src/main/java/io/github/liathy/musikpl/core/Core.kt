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
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.wifi.WifiManager
import android.provider.Settings
import java.net.Inet4Address
import uniffi.musikpl_mobile.AndroidPlatform
import uniffi.musikpl_mobile.ArtistState
import uniffi.musikpl_mobile.DevicesState
import uniffi.musikpl_mobile.DownloadsInfo
import uniffi.musikpl_mobile.FriendsState
import uniffi.musikpl_mobile.LyricsState
import uniffi.musikpl_mobile.JamRole
import uniffi.musikpl_mobile.JamState
import uniffi.musikpl_mobile.setDeviceName
import uniffi.musikpl_mobile.setNetworks
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
    private val _jam = MutableStateFlow<JamState?>(null)
    private val _friends = MutableStateFlow<FriendsState?>(null)
    private val _devices = MutableStateFlow<DevicesState?>(null)

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
    val jam: StateFlow<JamState?> = _jam
    val friends: StateFlow<FriendsState?> = _friends
    val devices: StateFlow<DevicesState?> = _devices
    private val _downloads = MutableStateFlow<DownloadsInfo?>(null)
    val downloads: StateFlow<DownloadsInfo?> = _downloads

    private val _lyrics = MutableStateFlow<LyricsState?>(null)
    private val _lyricsLine = MutableStateFlow<Int?>(null)
    val lyrics: StateFlow<LyricsState?> = _lyrics
    val lyricsLine: StateFlow<Int?> = _lyricsLine
    private var lyricsKey = ""

    /** The lyrics sheet shows: the engine looks them up, line by line. */
    @Volatile
    var lyricsVisible = false
        set(value) {
            field = value
            act { showLyrics(value) }
        }

    /** The Jam screen shows: find nearby Jams (needs the multicast lock). */
    @Volatile
    var jamVisible = false
        set(value) {
            field = value
            updateMulticast()
        }

    private var multicast: WifiManager.MulticastLock? = null

    /** The screens are showing: keep their snapshots fresh. */
    @Volatile
    var visible = false
        set(value) {
            field = value
            if (value) wake()
            // The user's other devices are found while the app is open.
            main.post { updateMulticast() }
        }

    /** Text the engine asked to copy (a friend code). */
    val clipboard = MutableStateFlow<String?>(null)

    fun init(context: Context) {
        app = context.applicationContext
        // Jams and friends show the phone's own name ("Galaxy A56").
        val name = Settings.Global.getString(app.contentResolver, "device_name")
            ?: android.os.Build.MODEL
        setDeviceName(name)
        watchNetworks()
        multicast = app.getSystemService(WifiManager::class.java)
            ?.createMulticastLock("musikpl-jam")
            ?.apply { setReferenceCounted(false) }
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
            val wait = if (lyricsVisible) minOf(next, 100L) else if (visible) minOf(next, 250L) else next
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
        _friends.value = engine.friends()
        _devices.value = engine.devices()
        _downloads.value = engine.downloads()
        if (lyricsVisible) {
            // The lines only when they change; the current line each time.
            val key = engine.lyricsKey()
            if (key != lyricsKey) {
                lyricsKey = key
                _lyrics.value = engine.lyrics()
            }
            _lyricsLine.value = engine.lyricsLine()?.toInt()
        }
        val jam = engine.jam()
        if (jam.role != _jam.value?.role) main.post { updateMulticast() }
        _jam.value = jam
    }

    /** Nearby Jams arrive as broadcasts, which Android drops without a lock. */
    private fun updateMulticast() {
        val lock = multicast ?: return
        val inJam = _jam.value?.role?.let { it != JamRole.NONE } == true
        if (jamVisible || inJam || visible) {
            if (!lock.isHeld) lock.acquire()
        } else if (lock.isHeld) {
            lock.release()
        }
    }

    /** Tell the engine the phone's networks, now and whenever they change. */
    private fun watchNetworks() {
        val connectivity = app.getSystemService(ConnectivityManager::class.java) ?: return
        val publish = { properties: LinkProperties? ->
            val networks = properties?.linkAddresses.orEmpty()
                .filter { it.address is Inet4Address && !it.address.isLoopbackAddress }
                .map { "${it.address.hostAddress}/${it.prefixLength}" }
            setNetworks(networks)
        }
        publish(connectivity.getLinkProperties(connectivity.activeNetwork))
        connectivity.registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
            override fun onLinkPropertiesChanged(network: Network, properties: LinkProperties) {
                publish(properties)
            }
        })
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
