@file:OptIn(ExperimentalLayoutApi::class)

package io.github.liathy.musikpl.ui

import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.liathy.musikpl.core.Core

@Composable
fun ArtistScreen(nav: Nav) {
    val artist by Core.artist.collectAsStateWithLifecycle()
    val state = artist ?: return Loading()
    var bioOpen by remember { mutableStateOf(false) }

    LazyColumn(contentPadding = PaddingValues(bottom = 16.dp)) {
        item {
            Column(
                Modifier.fillMaxWidth().padding(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                RoundPhoto(state.photo, state.name, Modifier.size(160.dp))
                Spacer(Modifier.size(12.dp))
                ScrollingText(state.name, MaterialTheme.typography.headlineMedium)
                if (state.listeners > 0u) {
                    Text(
                        "${compact(state.listeners)} listeners · ${compact(state.plays)} plays on Last.fm",
                        style = MaterialTheme.typography.bodyMedium,
                        color = Palette.secondary,
                    )
                }
                Spacer(Modifier.size(12.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Pill(
                        if (state.bookmarked) "✓ Bookmarked" else "Bookmark",
                        { Core.act { toggleBookmark() } },
                        primary = !state.bookmarked,
                    )
                    if (state.url.startsWith("https://")) {
                        Pill("Open on Last.fm", { Core.act { openLink(state.url) } }, primary = false)
                    }
                }
            }
        }
        if (state.loading) item { Loading(Modifier.fillMaxWidth().size(80.dp)) }
        state.error?.let { error ->
            item {
                Text(error, color = Palette.danger, modifier = Modifier.padding(16.dp))
            }
        }
        if (state.tags.isNotEmpty()) {
            item {
                FlowRow(
                    Modifier.padding(horizontal = 16.dp),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    state.tags.forEach { tag ->
                        Box(
                            Modifier.clip(RoundedCornerShape(50)).background(Palette.surface)
                                .padding(horizontal = 12.dp, vertical = 4.dp),
                        ) { Text(tag, style = MaterialTheme.typography.bodySmall, color = Palette.secondary) }
                    }
                }
            }
        }
        if (state.bio.isNotEmpty()) {
            item {
                Text(
                    state.bio,
                    style = MaterialTheme.typography.bodyMedium,
                    color = Palette.secondary,
                    maxLines = if (bioOpen) Int.MAX_VALUE else 4,
                    modifier = Modifier
                        .padding(16.dp)
                        .animateContentSize()
                        .clickable { bioOpen = !bioOpen },
                )
            }
        }
        if (state.topSongs.isNotEmpty()) {
            item { SectionTitle("Top songs") }
            itemsIndexed(state.topSongs) { index, song ->
                LastFmRow(
                    song.title,
                    "${compact(song.plays)} plays",
                    onPlay = { Core.act { playLastfmSong(state.name, song.title) } },
                    onMore = { nav.lastFmMenu(state.name, song.title) },
                    leading = "${index + 1}",
                )
            }
        }
        if (state.similar.isNotEmpty()) {
            item { SectionTitle("Similar artists") }
            item {
                LazyRow(
                    contentPadding = PaddingValues(horizontal = 16.dp),
                    horizontalArrangement = Arrangement.spacedBy(16.dp),
                ) {
                    itemsIndexed(state.similar) { index, similar ->
                        Column(
                            Modifier.width(96.dp).clickable { nav.artist(similar.name) },
                            horizontalAlignment = Alignment.CenterHorizontally,
                        ) {
                            RoundPhoto(state.similarPhotos.getOrNull(index), similar.name, Modifier.size(84.dp))
                            Spacer(Modifier.size(6.dp))
                            ScrollingText(similar.name, MaterialTheme.typography.titleMedium)
                        }
                    }
                }
            }
        }
    }
}
