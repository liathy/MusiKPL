@file:OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)

package io.github.liathy.musikpl.ui

import android.content.Intent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.liathy.musikpl.R
import io.github.liathy.musikpl.core.Core
import uniffi.musikpl_mobile.PlayerState
import uniffi.musikpl_mobile.Screen
import uniffi.musikpl_mobile.Song

enum class Tab { Home, Search, Library, Jam, Settings }

/** What the screens can ask the app to do, besides engine calls. */
class Nav(
    val openDetail: () -> Unit,
    val songMenu: (Song) -> Unit,
    val lastFmMenu: (artist: String, title: String) -> Unit,
    val openPlayer: () -> Unit,
    val goTo: (Tab) -> Unit,
) {
    fun artist(name: String) {
        Core.act { openArtist(name) }
        openDetail()
    }

    fun songArtist(song: Song) {
        Core.act { openSongArtist(song) }
        openDetail()
    }

    fun profile(user: String?) {
        Core.act { if (user == null) openMyProfile() else openUserProfile(user) }
        openDetail()
    }

    fun playlist(id: Long) {
        Core.act { openPlaylist(id) }
        openDetail()
    }
}

private val DETAIL = setOf(Screen.PLAYLIST, Screen.ARTIST, Screen.PROFILE)

@Composable
fun App(onLeave: () -> Unit) {
    val player by Core.player.collectAsStateWithLifecycle()
    var tab by rememberSaveable { mutableStateOf(Tab.Home) }
    // An engine page (playlist, artist, profile) shows over the tab.
    var detail by rememberSaveable { mutableStateOf(false) }
    var playerOpen by rememberSaveable { mutableStateOf(false) }
    // Lyrics belong to the app, not the player: they stay open, and close
    // on back or a tab change, even when the player is closed.
    var lyricsOpen by rememberSaveable { mutableStateOf(false) }
    var menuSong by remember { mutableStateOf<Song?>(null) }
    var menuLastFm by remember { mutableStateOf<Pair<String, String>?>(null) }
    val snackbar = remember { SnackbarHostState() }

    val screen = player?.screen
    var lastScreen by remember { mutableStateOf(screen) }
    LaunchedEffect(screen) {
        // Back (or the engine) left the page: show the tab again.
        if (lastScreen in DETAIL && screen !in DETAIL) detail = false
        lastScreen = screen
    }

    val nav = remember {
        Nav(
            openDetail = { detail = true },
            songMenu = { menuSong = it },
            lastFmMenu = { artist, title -> menuLastFm = artist to title },
            openPlayer = { playerOpen = true },
            goTo = {
                tab = it
                detail = false
                playerOpen = false
                lyricsOpen = false
                if (it == Tab.Home) Core.act { goHome() }
            },
        )
    }

    BackHandler(enabled = true) {
        when {
            lyricsOpen -> lyricsOpen = false
            playerOpen -> playerOpen = false
            detail -> Core.act { goBack() }
            tab != Tab.Home -> nav.goTo(Tab.Home)
            else -> onLeave()
        }
    }

    val error = player?.error
    LaunchedEffect(error) {
        if (error != null) {
            snackbar.showSnackbar(error)
            Core.act { dismissError() }
        }
    }

    Scaffold(
        containerColor = Palette.background,
        snackbarHost = { SnackbarHost(snackbar) },
        bottomBar = {
            Column(Modifier.background(Palette.background).navigationBarsPadding()) {
                player?.let { state ->
                    if (state.song != null) MiniPlayer(state, nav)
                }
                BottomTabs(tab, detail, nav)
            }
        },
    ) { padding ->
        Box(
            Modifier
                .fillMaxSize()
                .padding(padding)
                .statusBarsPadding(),
        ) {
            if (detail && screen in DETAIL) {
                when (screen) {
                    Screen.PLAYLIST -> PlaylistScreen(nav)
                    Screen.ARTIST -> ArtistScreen(nav)
                    Screen.PROFILE -> ProfileScreen(nav)
                    else -> {}
                }
            } else {
                when (tab) {
                    Tab.Home -> HomeScreen(nav)
                    Tab.Search -> SearchScreen(nav)
                    Tab.Library -> LibraryScreen(nav)
                    Tab.Jam -> JamScreen(nav)
                    Tab.Settings -> SettingsScreen(nav)
                }
            }
        }
    }

    if (playerOpen) {
        PlayerSheet(nav, onClose = { playerOpen = false }, onLyrics = { lyricsOpen = true })
    }
    if (lyricsOpen) {
        LyricsSheet(onClose = { lyricsOpen = false })
    }
    InvitePopup()
    DeviceLinkPopup()
    UpdatePrompt()
    menuSong?.let { song -> SongMenu(song, nav, onClose = { menuSong = null }) }
    menuLastFm?.let { (artist, title) ->
        LastFmMenu(artist, title, onClose = { menuLastFm = null })
    }
}

@Composable
private fun BottomTabs(tab: Tab, detail: Boolean, nav: Nav) {
    val colors = NavigationBarItemDefaults.colors(
        selectedIconColor = Color.White,
        selectedTextColor = Color.White,
        unselectedIconColor = Palette.muted,
        unselectedTextColor = Palette.muted,
        indicatorColor = Palette.surface,
    )
    NavigationBar(containerColor = Palette.background, tonalElevation = 0.dp) {
        listOf(
            Triple(Tab.Home, R.drawable.ic_home, "Home"),
            Triple(Tab.Search, R.drawable.ic_search, "Search"),
            Triple(Tab.Library, R.drawable.ic_playlist, "Library"),
            Triple(Tab.Jam, R.drawable.ic_friends, "Jam"),
            Triple(Tab.Settings, R.drawable.ic_settings, "Settings"),
        ).forEach { (item, icon, label) ->
            NavigationBarItem(
                selected = tab == item && !detail,
                onClick = { nav.goTo(item) },
                icon = { AppIcon(icon, label, tint = Color.Unspecified) },
                label = { Text(label) },
                colors = colors,
            )
        }
    }
}

@Composable
private fun MiniPlayer(state: PlayerState, nav: Nav) {
    val song = state.song ?: return
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 8.dp)
            .clip(RoundedCornerShape(10.dp))
            .background(Palette.panel)
            .clickable { nav.openPlayer() },
    ) {
        Row(
            Modifier.swipeSongs(!state.guest).padding(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Cover(song.cover, Modifier.size(44.dp), corner = 6.dp)
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                ScrollingText(song.title, MaterialTheme.typography.titleMedium)
                ScrollingText(
                    if (state.loading) "Loading…" else song.artist,
                    MaterialTheme.typography.bodySmall,
                    color = Palette.secondary,
                )
            }
            IconAction(
                if (state.liked) R.drawable.ic_heart_filled else R.drawable.ic_heart,
                if (state.liked) "Unlike" else "Like",
                { Core.act { toggleLikeCurrent() } },
                tint = if (state.liked) Palette.like else Palette.icon,
            )
            IconAction(
                if (state.paused || state.loading) R.drawable.ic_play else R.drawable.ic_pause,
                if (state.paused) "Play" else "Pause",
                { Core.act { togglePause() } },
                size = 20.dp,
            )
        }
        val progress = if (state.duration > 0) (state.position / state.duration).toFloat() else 0f
        LinearProgressIndicator(
            progress = { progress },
            modifier = Modifier.fillMaxWidth().height(2.dp),
            color = Color.White,
            trackColor = Palette.surface,
            drawStopIndicator = {},
        )
    }
    Spacer(Modifier.height(4.dp))
}

/** A song's menu: like, add to the mix, add to a playlist, artist, share. */
@Composable
fun SongMenu(song: Song, nav: Nav, onClose: () -> Unit) {
    val liked by Core.liked.collectAsStateWithLifecycle()
    val player by Core.player.collectAsStateWithLifecycle()
    var picking by remember { mutableStateOf(false) }
    val context = LocalContext.current
    val isLiked = liked.any { it.id == song.id }
    ModalBottomSheet(onDismissRequest = onClose, containerColor = Palette.panel) {
        Column(Modifier.verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                Cover(song.cover, Modifier.size(56.dp))
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    ScrollingText(song.title, MaterialTheme.typography.titleMedium)
                    ScrollingText(song.artist, MaterialTheme.typography.bodySmall, color = Palette.secondary)
                }
            }
            HorizontalDivider(color = Palette.surface)
            if (picking) {
                PlaylistPicker(
                    onPick = { id ->
                        Core.act { addToPlaylist(song, id) }
                        onClose()
                    },
                )
            } else {
                MenuItem(R.drawable.ic_play, "Play") {
                    Core.act { play(song) }
                    onClose()
                }
                MenuItem(
                    if (isLiked) R.drawable.ic_heart_filled else R.drawable.ic_heart,
                    if (isLiked) "Unlike" else "Like",
                    tint = if (isLiked) Palette.like else Palette.icon,
                ) {
                    Core.act { toggleLike(song) }
                    onClose()
                }
                MenuItem(R.drawable.ic_queue, if (player?.guest == true) "Add to the Jam queue" else "Add to mix") {
                    Core.act { addToMix(song) }
                    onClose()
                }
                MenuItem(R.drawable.ic_playlist_add, "Add to playlist") { picking = true }
                val downloads by Core.downloads.collectAsStateWithLifecycle()
                val saved = downloads?.savedIds?.contains(song.id) == true
                val pending = downloads?.pendingIds?.contains(song.id) == true
                MenuItem(
                    R.drawable.ic_download,
                    when {
                        saved -> "Remove download"
                        pending -> "Cancel download"
                        else -> "Download"
                    },
                    tint = if (saved) Palette.success else Palette.icon,
                ) {
                    Core.act { toggleDownload(song) }
                    onClose()
                }
                MenuItem(R.drawable.ic_artist, "Go to artist") {
                    nav.songArtist(song)
                    onClose()
                }
                MenuItem(R.drawable.ic_forward, "Share") {
                    val send = Intent(Intent.ACTION_SEND)
                        .setType("text/plain")
                        .putExtra(Intent.EXTRA_TEXT, "https://www.youtube.com/watch?v=${song.id}")
                    context.startActivity(Intent.createChooser(send, song.title))
                    onClose()
                }
            }
        }
    }
}

/** A Last.fm song's menu (artist pages, profiles): it's found on YouTube first. */
@Composable
fun LastFmMenu(artist: String, title: String, onClose: () -> Unit) {
    var picking by remember { mutableStateOf(false) }
    ModalBottomSheet(onDismissRequest = onClose, containerColor = Palette.panel) {
        Column(Modifier.verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            Column(Modifier.padding(16.dp)) {
                ScrollingText(title, MaterialTheme.typography.titleMedium)
                ScrollingText(artist, MaterialTheme.typography.bodySmall, color = Palette.secondary)
            }
            HorizontalDivider(color = Palette.surface)
            if (picking) {
                PlaylistPicker(onPick = { id ->
                    Core.act { addLastfmSongToPlaylist(artist, title, id) }
                    onClose()
                })
            } else {
                MenuItem(R.drawable.ic_play, "Play") {
                    Core.act { playLastfmSong(artist, title) }
                    onClose()
                }
                MenuItem(R.drawable.ic_heart, "Like") {
                    Core.act { likeLastfmSong(artist, title) }
                    onClose()
                }
                MenuItem(R.drawable.ic_queue, "Add to mix") {
                    Core.act { addLastfmSong(artist, title) }
                    onClose()
                }
                MenuItem(R.drawable.ic_playlist_add, "Add to playlist") { picking = true }
            }
        }
    }
}

/** Existing playlists, and a new one. `onPick(null)` makes a new playlist. */
@Composable
private fun PlaylistPicker(onPick: (Long?) -> Unit) {
    val playlists by Core.playlists.collectAsStateWithLifecycle()
    Text(
        "Add to playlist",
        style = MaterialTheme.typography.bodySmall,
        color = Palette.muted,
        modifier = Modifier.padding(start = 16.dp, top = 12.dp, bottom = 4.dp),
    )
    MenuItem(R.drawable.ic_add, "New playlist") { onPick(null) }
    playlists.forEach { playlist ->
        MenuItem(R.drawable.ic_playlist, playlist.name) { onPick(playlist.id) }
    }
}

@Composable
fun MenuItem(icon: Int, text: String, tint: Color = Palette.icon, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 20.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        AppIcon(icon, null, tint = tint, size = 20.dp)
        Spacer(Modifier.width(16.dp))
        Text(text, style = MaterialTheme.typography.bodyLarge)
    }
}

/** Ask for a name (new playlist, rename). */
@Composable
fun NameDialog(title: String, initial: String, onDone: (String) -> Unit, onCancel: () -> Unit) {
    var name by remember { mutableStateOf(initial) }
    AlertDialog(
        onDismissRequest = onCancel,
        containerColor = Palette.panel,
        title = { Text(title, style = MaterialTheme.typography.titleLarge) },
        text = {
            OutlinedTextField(value = name, onValueChange = { name = it }, singleLine = true)
        },
        confirmButton = {
            TextButton(onClick = { onDone(name) }, enabled = name.isNotBlank()) { Text("Save") }
        },
        dismissButton = { TextButton(onClick = onCancel) { Text("Cancel") } },
    )
}

/** A new version was found on start: offer it once. */
@Composable
private fun UpdatePrompt() {
    val update by io.github.liathy.musikpl.update.Updater.state.collectAsStateWithLifecycle()
    var dismissed by rememberSaveable { mutableStateOf(false) }
    val context = LocalContext.current
    val available = update as? io.github.liathy.musikpl.update.UpdateState.Available ?: return
    if (dismissed) return
    AlertDialog(
        onDismissRequest = { dismissed = true },
        containerColor = Palette.panel,
        title = { Text("MusiKPL ${available.update.version} is out", style = MaterialTheme.typography.titleLarge) },
        text = { Text("Update now? Your playlists, likes and settings stay.", color = Palette.secondary) },
        confirmButton = {
            TextButton(onClick = {
                dismissed = true
                io.github.liathy.musikpl.update.Updater.install(context, available.update)
            }) { Text("Update", color = Color.White) }
        },
        dismissButton = { TextButton(onClick = { dismissed = true }) { Text("Later") } },
    )
}
