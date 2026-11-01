@file:OptIn(ExperimentalMaterial3Api::class)

package io.github.liathy.musikpl.ui

import android.content.Context
import android.net.Uri
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
import java.io.File
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

    val pickPhoto = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        val copy = copyToCache(context, uri) ?: return@rememberLauncherForActivityResult
        Core.act { setPlaylistCover(playlist.id, copy.absolutePath); copy.delete() }
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
                            pickPhoto.launch(
                                PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly),
                            )
                        },
                    corner = 12.dp,
                )
                Spacer(Modifier.size(12.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    ScrollingText(
                        playlist.name,
                        MaterialTheme.typography.headlineMedium,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    IconAction(R.drawable.ic_edit, "Rename", { renaming = true }, size = 18.dp)
                }
                Text(
                    "${playlist.songs.size} songs",
                    style = MaterialTheme.typography.bodyMedium,
                    color = Palette.secondary,
                )
                Spacer(Modifier.size(12.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    if (playlist.songs.isNotEmpty()) {
                        Pill("Play", { Core.act { playPlaylist(playlist.id, 0u) } })
                    }
                    IconAction(
                        if (playlist.favorite) R.drawable.ic_heart_filled else R.drawable.ic_heart,
                        if (playlist.favorite) "Remove from Home" else "Show on Home",
                        { Core.act { togglePlaylistFavorite(playlist.id) } },
                        tint = if (playlist.favorite) Palette.like else Palette.icon,
                    )
                    if (playlist.cover != null) {
                        Pill("Remove photo", { Core.act { removePlaylistCover(playlist.id) } }, primary = false)
                    }
                    IconAction(R.drawable.ic_close, "Delete playlist", { deleting = true }, tint = Palette.danger)
                }
            }
        }
        itemsIndexed(playlist.songs, key = { _, song -> "song:" + song.id }) { index, song ->
            SongRow(
                song,
                onPlay = { Core.act { playPlaylist(playlist.id, index.toUInt()) } },
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
                    Core.act { playPlaylist(playlist.id, index.toUInt()) }
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

/** The picked photo, copied to a file the core can read. */
private fun copyToCache(context: Context, uri: Uri): File? = runCatching {
    val file = File(context.cacheDir, "cover-${System.currentTimeMillis()}")
    context.contentResolver.openInputStream(uri)!!.use { input ->
        file.outputStream().use { input.copyTo(it) }
    }
    file
}.getOrNull()
