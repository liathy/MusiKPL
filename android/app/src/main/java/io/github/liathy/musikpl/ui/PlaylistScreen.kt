@file:OptIn(ExperimentalMaterial3Api::class)

package io.github.liathy.musikpl.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import uniffi.musikpl_mobile.FadeShape
import uniffi.musikpl_mobile.TransitionInfo
import uniffi.musikpl_mobile.TransitionKind
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.liathy.musikpl.R
import io.github.liathy.musikpl.core.Core
import uniffi.musikpl_mobile.Song

@Composable
fun PlaylistScreen(nav: Nav) {
    val detail by Core.playlist.collectAsStateWithLifecycle()
    val player by Core.player.collectAsStateWithLifecycle()
    val liked by Core.liked.collectAsStateWithLifecycle()
    val likedIds = liked.map { it.id }.toSet()
    val playlist = detail ?: return Loading()
    val context = LocalContext.current
    var renaming by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf(false) }
    var songMenu by remember { mutableStateOf<Pair<Int, Song>?>(null) }
    var coverMenu by remember { mutableStateOf(false) }
    var transitions by remember { mutableStateOf(false) }

    // A picked photo waits in the crop step until saved.
    var cropping by remember { mutableStateOf<android.graphics.Bitmap?>(null) }
    val pickPhoto = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        cropping = readPhoto(context, uri)
        if (cropping == null) Core.act { setPlaylistCover(playlist.id, "") }
    }

    val list = rememberLazyListState()
    LaunchedEffect(list, playlist.suggestions.size) {
        snapshotFlow { list.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0 }
            .collect { last ->
                if (last >= list.layoutInfo.totalItemsCount - 2) Core.act { morePlaylistSuggestions() }
            }
    }

    LazyColumn(state = list, contentPadding = PaddingValues(bottom = 16.dp)) {
        item {
            Column(
                Modifier.fillMaxWidth().padding(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Cover(
                    playlist.cover,
                    Modifier
                        .size(180.dp)
                        .clickable {
                            if (playlist.cover != null) {
                                coverMenu = true
                            } else {
                                pickPhoto.launch(
                                    PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly),
                                )
                            }
                        },
                    corner = 12.dp,
                )
                Spacer(Modifier.size(12.dp))
                ScrollingText(
                    playlist.name,
                    MaterialTheme.typography.headlineMedium,
                    modifier = Modifier.clickable { renaming = true },
                )
                Text(
                    (if (playlist.songs.size == 1) "1 song" else "${playlist.songs.size} songs") +
                        if (playlist.downloaded) " · ${playlist.saved} downloaded" else "",
                    style = MaterialTheme.typography.bodyMedium,
                    color = Palette.secondary,
                )
                Spacer(Modifier.size(12.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    if (playlist.songs.isNotEmpty()) {
                        Pill("Play", { Core.act { playPlaylist(playlist.id) } })
                        IconAction(
                            R.drawable.ic_shuffle,
                            "Shuffle play",
                            {
                                Core.act {
                                    if (playerState().shuffle == uniffi.musikpl_mobile.Shuffle.OFF) cycleShuffle()
                                    playPlaylist(playlist.id)
                                }
                            },
                        )
                    }
                    IconAction(
                        R.drawable.ic_download,
                        if (playlist.downloaded) "Remove download" else "Download",
                        { Core.act { togglePlaylistDownload(playlist.id) } },
                        tint = if (playlist.downloaded) Palette.success else Palette.icon,
                        size = 22.dp,
                    )
                    IconAction(R.drawable.ic_crossfade, "Transitions", { transitions = true }, size = 22.dp)
                    IconAction(R.drawable.ic_edit, "Rename", { renaming = true }, size = 20.dp)
                    IconAction(
                        if (playlist.favorite) R.drawable.ic_heart_filled else R.drawable.ic_heart,
                        if (playlist.favorite) "Remove from Home" else "Show on Home",
                        { Core.act { togglePlaylistFavorite(playlist.id) } },
                        tint = if (playlist.favorite) Palette.like else Palette.icon,
                    )

                    IconAction(R.drawable.ic_close, "Delete playlist", { deleting = true }, tint = Palette.danger)
                }
            }
        }
        itemsIndexed(playlist.songs, key = { _, song -> "song:" + song.id }) { index, song ->
            SongRow(
                song,
                onPlay = { Core.act { playPlaylistFrom(playlist.id, index.toUInt()) } },
                onMore = { songMenu = index to song },
                playing = player?.song?.id == song.id,
                liked = song.id in likedIds,
                leading = "${index + 1}",
            )
        }
        if (playlist.songs.isNotEmpty() && (playlist.suggestions.isNotEmpty() || playlist.suggestionsLoading)) {
            item { SectionTitle("Recommended songs") }
            items(playlist.suggestions, key = { "rec:" + it.id }) { song ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    SongRow(
                        song,
                        onPlay = { Core.act { play(song) } },
                        onMore = { nav.songMenu(song) },
                        modifier = Modifier.weight(1f),
                    )
                }
            }
            if (playlist.suggestionsLoading) item { Loading(Modifier.fillMaxWidth().size(60.dp)) }
        }
    }

    songMenu?.let { (index, song) ->
        ModalBottomSheet(onDismissRequest = { songMenu = null }, containerColor = Palette.panel) {
            Column(Modifier.verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
                MenuItem(R.drawable.ic_play, "Play from here") {
                    Core.act { playPlaylistFrom(playlist.id, index.toUInt()) }
                    songMenu = null
                }
                if (index > 0) {
                    MenuItem(R.drawable.ic_chevron, "Move up") {
                        Core.act { moveInPlaylist(playlist.id, index.toUInt(), (index - 1).toUInt()) }
                        songMenu = null
                    }
                }
                if (index < playlist.songs.size - 1) {
                    MenuItem(R.drawable.ic_chevron, "Move down") {
                        Core.act { moveInPlaylist(playlist.id, index.toUInt(), (index + 1).toUInt()) }
                        songMenu = null
                    }
                }
                MenuItem(R.drawable.ic_close, "Remove from this playlist", tint = Palette.danger) {
                    Core.act { removeFromPlaylist(playlist.id, index.toUInt()) }
                    songMenu = null
                }
                MenuItem(R.drawable.ic_add, "More…") {
                    songMenu = null
                    nav.songMenu(song)
                }
            }
        }
    }
    if (coverMenu) {
        ModalBottomSheet(onDismissRequest = { coverMenu = false }, containerColor = Palette.panel) {
            Column(Modifier.padding(bottom = 24.dp)) {
                MenuItem(R.drawable.ic_edit, "Choose another photo") {
                    coverMenu = false
                    pickPhoto.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
                }
                MenuItem(R.drawable.ic_close, "Remove photo", tint = Palette.danger) {
                    coverMenu = false
                    Core.act { removePlaylistCover(playlist.id) }
                }
            }
        }
    }
    cropping?.let { photo ->
        CoverCropDialog(
            photo,
            onDone = { square ->
                cropping = null
                val file = saveSquare(context, square)
                Core.act {
                    setPlaylistCover(playlist.id, file.absolutePath)
                    file.delete()
                }
            },
            onCancel = { cropping = null },
        )
    }
    if (transitions) {
        TransitionsSheet(playlist.id, playlist.transition) { transitions = false }
    }
    if (renaming) {
        NameDialog("Rename playlist", playlist.name, onDone = { name ->
            renaming = false
            Core.act { renamePlaylist(playlist.id, name) }
        }, onCancel = { renaming = false })
    }
    if (deleting) {
        AlertDialog(
            onDismissRequest = { deleting = false },
            containerColor = Palette.panel,
            title = { Text("Delete “${playlist.name}”?", style = MaterialTheme.typography.titleLarge) },
            text = { Text("The songs stay on YouTube; only this playlist goes.") },
            confirmButton = {
                TextButton(onClick = {
                    deleting = false
                    Core.act { deletePlaylist(playlist.id) }
                }) { Text("Delete", color = Palette.danger) }
            },
            dismissButton = { TextButton(onClick = { deleting = false }) { Text("Cancel") } },
        )
    }
}

/** How a playlist's songs fade into each other. */
@Composable
private fun TransitionsSheet(id: Long, transition: TransitionInfo, onClose: () -> Unit) {
    fun set(kind: TransitionKind, seconds: UByte = transition.seconds, curve: FadeShape = transition.curve) {
        Core.act { setPlaylistTransition(id, kind, seconds, curve) }
    }
    ModalBottomSheet(onDismissRequest = onClose, containerColor = Palette.panel) {
        Column(Modifier.padding(start = 20.dp, end = 20.dp, bottom = 32.dp)) {
            Text("Transitions", style = MaterialTheme.typography.titleLarge)
            Text(
                "How this playlist's songs fade into each other.",
                style = MaterialTheme.typography.bodySmall,
                color = Palette.secondary,
            )
            Spacer(Modifier.size(16.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Pill("As in Settings", { set(TransitionKind.DEFAULT) }, primary = transition.kind == TransitionKind.DEFAULT)
                Pill("No fade", { set(TransitionKind.OFF) }, primary = transition.kind == TransitionKind.OFF)
                Pill("Its own", { set(TransitionKind.FADE) }, primary = transition.kind == TransitionKind.FADE)
            }
            if (transition.kind == TransitionKind.FADE) {
                Spacer(Modifier.size(16.dp))
                FadeControls(transition.seconds, transition.curve) { seconds, curve ->
                    set(TransitionKind.FADE, seconds, curve)
                }
            }
        }
    }
}