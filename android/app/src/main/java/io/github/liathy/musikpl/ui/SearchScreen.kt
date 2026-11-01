package io.github.liathy.musikpl.ui

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
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.liathy.musikpl.R
import io.github.liathy.musikpl.core.Core

@Composable
fun SearchScreen(nav: Nav) {
    val search by Core.search.collectAsStateWithLifecycle()
    val player by Core.player.collectAsStateWithLifecycle()
    val liked by Core.liked.collectAsStateWithLifecycle()
    val likedIds = liked.map { it.id }.toSet()
    var text by rememberSaveable { mutableStateOf(search?.query.orEmpty()) }
    val focus = LocalFocusManager.current
    val keyboard = LocalSoftwareKeyboardController.current
    val go = { query: String ->
        text = query
        focus.clearFocus()
        keyboard?.hide()
        if (query.isNotBlank()) Core.act { search(query) }
    }
    val state = search

    LazyColumn(contentPadding = PaddingValues(bottom = 16.dp)) {
        item {
            TextField(
                value = text,
                onValueChange = { text = it },
                placeholder = { Text("Songs, artists, or a YouTube link") },
                leadingIcon = { AppIcon(R.drawable.ic_search, null, tint = Palette.secondary) },
                trailingIcon = {
                    if (text.isNotEmpty()) IconAction(R.drawable.ic_close, "Clear", { text = "" })
                },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { go(text) }),
                shape = RoundedCornerShape(50),
                colors = TextFieldDefaults.colors(
                    focusedContainerColor = Palette.surface,
                    unfocusedContainerColor = Palette.surface,
                    focusedIndicatorColor = Color.Transparent,
                    unfocusedIndicatorColor = Color.Transparent,
                    cursorColor = Color.White,
                ),
                modifier = Modifier.fillMaxWidth().padding(16.dp),
            )
        }
        if (state == null || state.query.isEmpty()) {
            val recent = state?.recent.orEmpty().take(8)
            if (recent.isNotEmpty()) {
                item { SectionTitle("Recent searches") }
                items(recent) { query ->
                    Row(
                        Modifier.fillMaxWidth().clickable { go(query) }.padding(horizontal = 16.dp, vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        AppIcon(R.drawable.ic_history, null, tint = Palette.secondary, size = 18.dp)
                        Spacer(Modifier.width(14.dp))
                        ScrollingText(query, MaterialTheme.typography.bodyLarge)
                    }
                }
            }
            return@LazyColumn
        }
        if (state.people.isNotEmpty() || state.artists.isNotEmpty()) {
            item { SectionTitle(if (state.people.isNotEmpty()) "Artists and people" else "Artists") }
            item {
                LazyRow(
                    contentPadding = PaddingValues(horizontal = 16.dp),
                    horizontalArrangement = Arrangement.spacedBy(16.dp),
                ) {
                    items(state.people) { person ->
                        PersonChip(person.name, person.avatar, "${compact(person.scrobbles)} scrobbles") {
                            nav.profile(person.name)
                        }
                    }
                    items(state.artists.take(8)) { artist ->
                        PersonChip(artist.name, null, "${compact(artist.listeners)} listeners") {
                            nav.artist(artist.name)
                        }
                    }
                }
            }
        }
        item {
            SectionTitle("Songs") { if (state.searching || state.findingSong) Loading(Modifier.size(40.dp)) }
        }
        if (state.songs.isEmpty() && !state.searching && state.lastfmSongs.isNotEmpty()) {
            item {
                Text(
                    "YouTube's search found nothing, so these come from Last.fm. Picking one finds it on YouTube.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = Palette.secondary,
                    modifier = Modifier.padding(horizontal = 16.dp),
                )
            }
            items(state.lastfmSongs) { song ->
                LastFmRow(
                    song.title,
                    "${song.artist} · ${compact(song.listeners)} listeners",
                    onPlay = { Core.act { playLastfmSong(song.artist, song.title) } },
                    onMore = { nav.lastFmMenu(song.artist, song.title) },
                )
            }
        } else if (state.songs.isEmpty() && !state.searching) {
            item {
                Text(
                    "No results for “${state.query}”.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = Palette.secondary,
                    modifier = Modifier.padding(16.dp),
                )
            }
        }
        items(state.songs, key = { it.id }) { song ->
            SongRow(
                song,
                onPlay = { Core.act { play(song) } },
                onMore = { nav.songMenu(song) },
                playing = player?.song?.id == song.id,
                liked = song.id in likedIds,
            )
        }
    }
}

@Composable
private fun PersonChip(name: String, photo: String?, detail: String, onClick: () -> Unit) {
    Column(
        Modifier.width(96.dp).clickable(onClick = onClick),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        RoundPhoto(photo, name, Modifier.size(84.dp))
        Spacer(Modifier.size(6.dp))
        ScrollingText(name, MaterialTheme.typography.titleMedium)
        ScrollingText(detail, MaterialTheme.typography.bodySmall, color = Palette.secondary)
    }
}
