package io.github.liathy.musikpl.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.liathy.musikpl.R
import io.github.liathy.musikpl.core.Core

/** Playlists, liked songs, and bookmarked artists. */
@Composable
fun LibraryScreen(nav: Nav) {
    val playlists by Core.playlists.collectAsStateWithLifecycle()
    val liked by Core.liked.collectAsStateWithLifecycle()
    val settings by Core.settings.collectAsStateWithLifecycle()
    val player by Core.player.collectAsStateWithLifecycle()
    var naming by remember { mutableStateOf(false) }
    var showAllLiked by remember { mutableStateOf(false) }

    LazyColumn(contentPadding = PaddingValues(bottom = 16.dp)) {
        item {
            Text(
                "Library",
                style = MaterialTheme.typography.headlineMedium,
                modifier = Modifier.padding(start = 16.dp, top = 12.dp),
            )
        }
        item {
            SectionTitle("Playlists") {
                IconAction(R.drawable.ic_add, "New playlist", { naming = true })
            }
        }
        if (playlists.isEmpty()) {
            item {
                Text(
                    "Make a playlist with +, or add songs to one from any song's menu.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = Palette.secondary,
                    modifier = Modifier.padding(horizontal = 16.dp),
                )
            }
        }
        items(playlists, key = { it.id }) { playlist ->
            Row(
                Modifier
                    .fillMaxWidth()
                    .clickable { nav.playlist(playlist.id) }
                    .padding(horizontal = 16.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (playlist.cover != null) {
                    Cover(playlist.cover, Modifier.size(52.dp))
                } else {
                    Cover(null, Modifier.size(52.dp))
                }
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    ScrollingText(playlist.name, MaterialTheme.typography.titleMedium)
                    Text(
                        "${playlist.songs} song${if (playlist.songs == 1u) "" else "s"}",
                        style = MaterialTheme.typography.bodySmall,
                        color = Palette.secondary,
                    )
                }
                IconAction(
                    if (playlist.favorite) R.drawable.ic_heart_filled else R.drawable.ic_heart,
                    if (playlist.favorite) "Remove from Home" else "Show on Home",
                    { Core.act { togglePlaylistFavorite(playlist.id) } },
                    tint = if (playlist.favorite) Palette.like else Palette.icon,
                )
            }
        }
        val bookmarks = settings?.bookmarks.orEmpty()
        if (bookmarks.isNotEmpty()) {
            item { SectionTitle("Artists") }
            items(bookmarks, key = { "artist:" + it.name }) { artist ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clickable { nav.artist(artist.name) }
                        .padding(horizontal = 16.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RoundPhoto(artist.photo, artist.name, Modifier.size(52.dp))
                    Spacer(Modifier.width(12.dp))
                    ScrollingText(artist.name, MaterialTheme.typography.titleMedium)
                }
            }
        }
        item { SectionTitle("Liked songs") }
        if (liked.isEmpty()) {
            item {
                Text(
                    "Songs you ♥ show up here.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = Palette.secondary,
                    modifier = Modifier.padding(horizontal = 16.dp),
                )
            }
        }
        val shown = if (showAllLiked) liked else liked.take(20)
        items(shown, key = { "liked:" + it.id }) { song ->
            SongRow(
                song,
                onPlay = { Core.act { play(song) } },
                onMore = { nav.songMenu(song) },
                playing = player?.song?.id == song.id,
                liked = true,
            )
        }
        if (liked.size > shown.size) {
            item {
                Text(
                    "Show all ${liked.size}",
                    style = MaterialTheme.typography.labelLarge,
                    color = Palette.secondary,
                    modifier = Modifier.clickable { showAllLiked = true }.padding(16.dp),
                )
            }
        }
    }

    if (naming) {
        NameDialog(
            title = "New playlist",
            initial = "",
            onDone = { name ->
                naming = false
                Core.act { createPlaylist(name) }
                nav.openDetail()
            },
            onCancel = { naming = false },
        )
    }
}
