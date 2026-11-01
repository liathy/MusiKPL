@file:OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)

package io.github.liathy.musikpl.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.liathy.musikpl.R
import io.github.liathy.musikpl.core.Core
import uniffi.musikpl_mobile.HomeRow
import uniffi.musikpl_mobile.HomeState

@Composable
fun HomeScreen(nav: Nav) {
    val home by Core.home.collectAsStateWithLifecycle()
    val state = home ?: return Loading()
    state.pickingGenres?.let { picks ->
        GenrePicker(state, picks)
        return
    }
    val liked by Core.liked.collectAsStateWithLifecycle()
    val likedIds = liked.map { it.id }.toSet()
    val list = rememberLazyListState()
    // Reaching the bottom adds rows (infinite scrolling, like the desktop).
    LaunchedEffect(list, state.rows.size) {
        snapshotFlow { list.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0 }
            .collect { last -> if (last >= state.rows.size - 1) Core.act { moreHomeRows() } }
    }
    PullToRefreshBox(
        isRefreshing = state.building,
        onRefresh = { Core.act { refreshHome() } },
        modifier = Modifier.fillMaxSize(),
    ) {
        LazyColumn(state = list, contentPadding = PaddingValues(bottom = 16.dp)) {
            item {
                Row(
                    Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Home", style = MaterialTheme.typography.headlineMedium, modifier = Modifier.weight(1f))
                    IconAction(R.drawable.ic_refresh, "Refresh", { Core.act { refreshHome() } })
                }
            }
            if (state.rows.isEmpty()) {
                item {
                    Text(
                        if (state.building) "Building your Home…" else "Play some songs and Home fills up with music for you.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = Palette.secondary,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.fillMaxWidth().padding(32.dp),
                    )
                }
            }
            items(state.rows, key = { it.title }) { row -> HomeRowView(row, likedIds, nav) }
            if (state.loadingRows) {
                item { Loading(Modifier.fillMaxWidth().height(80.dp)) }
            }
        }
    }
}

@Composable
private fun HomeRowView(row: HomeRow, liked: Set<String>, nav: Nav) {
    val list = rememberLazyListState()
    LaunchedEffect(list, row.songs.size) {
        snapshotFlow { list.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0 }
            .collect { last -> if (last >= row.songs.size - 3) Core.act { moreInHomeRow(row.title) } }
    }
    SectionTitle(row.title) {
        row.playlistId?.let { id ->
            Text(
                "Open",
                style = MaterialTheme.typography.labelLarge,
                color = Palette.secondary,
                modifier = Modifier.clickable { nav.playlist(id) }.padding(12.dp),
            )
        }
    }
    LazyRow(
        state = list,
        contentPadding = PaddingValues(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        items(row.songs, key = { it.id }) { song ->
            SongCard(
                song,
                onPlay = { Core.act { play(song) } },
                onMore = { nav.songMenu(song) },
                liked = song.id in liked,
            )
        }
        if (row.loading) {
            item { Loading(Modifier.size(148.dp)) }
        }
    }
}

/** First run (or Settings): pick favorite genres for Home. */
@Composable
private fun GenrePicker(state: HomeState, picks: List<String>) {
    LazyColumn(contentPadding = PaddingValues(16.dp)) {
        item {
            Text("What do you like?", style = MaterialTheme.typography.headlineMedium)
            Text(
                "Pick a few genres and Home starts with them. It learns the rest as you listen.",
                style = MaterialTheme.typography.bodyLarge,
                color = Palette.secondary,
                modifier = Modifier.padding(vertical = 8.dp),
            )
        }
        item {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                state.genres.forEach { genre ->
                    val picked = genre in picks
                    Box(
                        Modifier
                            .clip(RoundedCornerShape(50))
                            .background(if (picked) Color.White else Palette.surface)
                            .clickable { Core.act { toggleGenre(genre) } }
                            .padding(horizontal = 14.dp, vertical = 8.dp),
                    ) {
                        Text(
                            genre,
                            style = MaterialTheme.typography.labelLarge,
                            color = if (picked) Color.Black else Palette.text,
                        )
                    }
                }
            }
        }
        item {
            Spacer(Modifier.height(20.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Pill("Done", { Core.act { finishGenres() } })
                Pill("Skip", { Core.act { skipGenres() } }, primary = false)
            }
        }
    }
}

@Composable
fun Loading(modifier: Modifier = Modifier.fillMaxSize()) {
    Box(modifier, contentAlignment = Alignment.Center) {
        CircularProgressIndicator(color = Palette.secondary, strokeWidth = 2.dp, modifier = Modifier.size(28.dp))
    }
}
