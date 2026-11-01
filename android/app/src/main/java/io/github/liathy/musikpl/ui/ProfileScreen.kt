package io.github.liathy.musikpl.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.liathy.musikpl.core.Core

private val PERIODS = listOf("Last 7 days", "Last month", "Last year", "All time")

@Composable
fun ProfileScreen(nav: Nav) {
    val profile by Core.profile.collectAsStateWithLifecycle()
    val state = profile ?: return Loading()
    if (state.user.isEmpty()) {
        Column(Modifier.fillMaxWidth().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Text("Your Last.fm profile", style = MaterialTheme.typography.headlineMedium)
            Text(
                "Connect Last.fm in Settings to see your scrobbles, top artists and songs.",
                color = Palette.secondary,
                modifier = Modifier.padding(vertical = 12.dp),
            )
            Pill("Open Settings", { nav.goTo(Tab.Settings) })
        }
        return
    }
    LazyColumn(contentPadding = PaddingValues(bottom = 16.dp)) {
        item {
            Column(Modifier.fillMaxWidth().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                RoundPhoto(state.avatar, state.user, Modifier.size(120.dp))
                Spacer(Modifier.size(10.dp))
                ScrollingText(state.user, MaterialTheme.typography.headlineMedium)
                if (state.scrobbles > 0u) {
                    Text(
                        "${compact(state.scrobbles)} scrobbles · ${compact(state.artists)} artists · ${compact(state.loved)} loved",
                        style = MaterialTheme.typography.bodyMedium,
                        color = Palette.secondary,
                    )
                }
                if (state.url.startsWith("https://")) {
                    Spacer(Modifier.size(10.dp))
                    Pill("Open on Last.fm", { Core.act { openLink(state.url) } }, primary = false)
                }
            }
        }
        if (state.loading) item { Loading(Modifier.fillMaxWidth().size(60.dp)) }
        state.error?.let { item { Text(it, color = Palette.danger, modifier = Modifier.padding(16.dp)) } }
        item {
            LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                itemsIndexed(PERIODS) { index, label ->
                    val chosen = state.period.toInt() == index
                    Box(
                        Modifier.clip(RoundedCornerShape(50))
                            .background(if (chosen) Color.White else Palette.surface)
                            .clickable { Core.act { setProfilePeriod(index.toUByte()) } }
                            .padding(horizontal = 14.dp, vertical = 6.dp),
                    ) {
                        Text(label, style = MaterialTheme.typography.labelLarge, color = if (chosen) Color.Black else Palette.text)
                    }
                }
            }
        }
        if (state.topArtists.isNotEmpty()) {
            item { SectionTitle("Top artists") }
            item {
                LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    items(state.topArtists) { artist ->
                        Column(
                            Modifier.width(96.dp).clickable { nav.artist(artist.name) },
                            horizontalAlignment = Alignment.CenterHorizontally,
                        ) {
                            RoundPhoto(null, artist.name, Modifier.size(84.dp))
                            Spacer(Modifier.size(6.dp))
                            ScrollingText(artist.name, MaterialTheme.typography.titleMedium)
                            Text("${compact(artist.listeners)} plays", style = MaterialTheme.typography.bodySmall, color = Palette.secondary)
                        }
                    }
                }
            }
        }
        if (state.topSongs.isNotEmpty()) {
            item { SectionTitle("Top songs") }
            itemsIndexed(state.topSongs) { index, song ->
                LastFmRow(
                    song.title,
                    "${song.artist} · ${compact(song.plays)} plays",
                    onPlay = { Core.act { playLastfmSong(song.artist, song.title) } },
                    onMore = { nav.lastFmMenu(song.artist, song.title) },
                    leading = "${index + 1}",
                )
            }
        }
        if (state.recent.isNotEmpty()) {
            item { SectionTitle("Recently played") }
            items(state.recent) { song ->
                LastFmRow(
                    song.title,
                    if (song.nowPlaying) "${song.artist} · now playing" else song.artist,
                    onPlay = { Core.act { playLastfmSong(song.artist, song.title) } },
                    onMore = { nav.lastFmMenu(song.artist, song.title) },
                )
            }
        }
    }
}
