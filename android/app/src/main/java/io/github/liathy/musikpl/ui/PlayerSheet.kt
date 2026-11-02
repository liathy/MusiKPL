@file:OptIn(ExperimentalMaterial3Api::class)

package io.github.liathy.musikpl.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.liathy.musikpl.R
import io.github.liathy.musikpl.core.Core
import uniffi.musikpl_mobile.Repeat
import uniffi.musikpl_mobile.Shuffle

/** The full player: cover, controls, and Up next (the queue). */
@Composable
fun PlayerSheet(nav: Nav, onClose: () -> Unit, onLyrics: () -> Unit) {
    val player by Core.player.collectAsStateWithLifecycle()
    val queue by Core.queue.collectAsStateWithLifecycle()
    val state = player ?: return
    val song = state.song
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    // While dragging, the slider shows the finger, not the player.
    var dragging by remember { mutableStateOf<Float?>(null) }

    ModalBottomSheet(onDismissRequest = onClose, sheetState = sheet, containerColor = Palette.background) {
        LazyColumn(contentPadding = PaddingValues(bottom = 32.dp)) {
            item {
                Column(Modifier.padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Cover(song?.cover, Modifier.fillMaxWidth().aspectRatio(1f).swipeSongs(!state.guest), corner = 14.dp)
                    Spacer(Modifier.size(20.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            ScrollingText(song?.title ?: "Nothing playing", MaterialTheme.typography.headlineMedium)
                            ScrollingText(
                                if (state.loading) "Loading…" else song?.artist.orEmpty(),
                                MaterialTheme.typography.bodyLarge,
                                color = Palette.secondary,
                                modifier = Modifier.clickable(enabled = song != null) {
                                    song?.let { nav.songArtist(it) }
                                    onClose()
                                },
                            )
                        }
                        IconAction(
                            if (state.liked) R.drawable.ic_heart_filled else R.drawable.ic_heart,
                            if (state.liked) "Unlike" else "Like",
                            { Core.act { toggleLikeCurrent() } },
                            tint = if (state.liked) Palette.like else Palette.icon,
                            size = 26.dp,
                        )
                        if (song != null) {
                            IconAction(R.drawable.ic_lyrics, "Lyrics", onLyrics, size = 26.dp)
                            IconAction(R.drawable.ic_playlist_add, "Add to playlist", { nav.songMenu(song) }, size = 26.dp)
                        }
                    }
                    val duration = state.duration.toFloat()
                    Slider(
                        value = dragging ?: state.position.toFloat().coerceIn(0f, duration.coerceAtLeast(0f)),
                        onValueChange = { dragging = it },
                        onValueChangeFinished = {
                            dragging?.let { position -> Core.act { seek(position.toDouble()) } }
                            dragging = null
                        },
                        valueRange = 0f..duration.coerceAtLeast(1f),
                        enabled = duration > 0 && !state.guest,
                        colors = SliderDefaults.colors(
                            thumbColor = Color.White,
                            activeTrackColor = Color.White,
                            inactiveTrackColor = Palette.surface,
                        ),
                    )
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Text(clock((dragging ?: state.position.toFloat()).toDouble()), style = MaterialTheme.typography.bodySmall, color = Palette.secondary)
                        Text(clock(state.duration), style = MaterialTheme.typography.bodySmall, color = Palette.secondary)
                    }
                    Row(
                        Modifier.fillMaxWidth().padding(vertical = 8.dp),
                        horizontalArrangement = Arrangement.SpaceEvenly,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        ModeButton(
                            icon = if (state.shuffle == Shuffle.SMART) R.drawable.ic_shuffle_smart else R.drawable.ic_shuffle,
                            label = when (state.shuffle) {
                                Shuffle.OFF -> "Shuffle"
                                Shuffle.ON -> "Shuffle on (tap for Smart Shuffle)"
                                Shuffle.SMART -> "Smart Shuffle (tap to turn off)"
                            },
                            on = state.shuffle != Shuffle.OFF,
                            enabled = !state.guest,
                        ) { Core.act { cycleShuffle() } }
                        IconAction(R.drawable.ic_previous, "Previous", { Core.act { previous() } }, size = 28.dp)
                        IconAction(
                            if (state.paused || state.loading) R.drawable.ic_play else R.drawable.ic_pause,
                            if (state.paused) "Play" else "Pause",
                            { Core.act { togglePause() } },
                            modifier = Modifier.size(72.dp),
                            size = 36.dp,
                        )
                        IconAction(R.drawable.ic_next, "Next", { Core.act { next() } }, size = 28.dp)
                        ModeButton(
                            icon = if (state.repeat == Repeat.ONE) R.drawable.ic_repeat_one else R.drawable.ic_repeat,
                            label = when (state.repeat) {
                                Repeat.OFF -> "Repeat"
                                Repeat.ALL -> "Repeating all (tap to repeat this song)"
                                Repeat.ONE -> "Repeating this song (tap to turn off)"
                            },
                            on = state.repeat != Repeat.OFF,
                            enabled = !state.guest,
                        ) { Core.act { cycleRepeat() } }
                    }
                }
            }
            val songs = queue?.songs.orEmpty()
            item {
                SectionTitle(if (state.guest) "Jam queue" else "Up next") {
                    if (queue?.mixing == true) Loading(Modifier.size(40.dp))
                }
            }
            if (songs.isEmpty()) {
                item {
                    Text(
                        if (queue?.autoMix == true) "Auto Mix adds songs like this one." else "Nothing queued.",
                        color = Palette.secondary,
                        modifier = Modifier.padding(horizontal = 16.dp),
                    )
                }
            }
            itemsIndexed(songs, key = { _, it -> "queue:" + it.id }) { index, queued ->
                if (queued.id in queue?.smart.orEmpty()) {
                    Text(
                        "✦ Smart Shuffle pick",
                        style = MaterialTheme.typography.bodySmall,
                        color = Palette.muted,
                        modifier = Modifier.padding(start = 16.dp, top = 6.dp),
                    )
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    SongRow(
                        queued,
                        onPlay = { if (!state.guest) Core.act { playFromQueue(queued) } },
                        onMore = { nav.songMenu(queued) },
                        modifier = Modifier.weight(1f),
                    )
                    if (!state.guest) {
                        IconAction(R.drawable.ic_close, "Remove", { Core.act { removeFromQueue(index.toUInt()) } })
                    }
                }
            }
        }
    }
}

/** Shuffle or repeat: dim when off, white with a dot under it when on. */
@Composable
private fun ModeButton(icon: Int, label: String, on: Boolean, enabled: Boolean, onClick: () -> Unit) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        IconAction(
            icon,
            label,
            { if (enabled) onClick() },
            tint = if (on) Color.White else Palette.secondary,
            size = 24.dp,
        )
        Box(
            Modifier
                .size(4.dp)
                .clip(CircleShape)
                .background(if (on) Color.White else Color.Transparent),
        )
    }
}
