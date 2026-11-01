@file:OptIn(ExperimentalMaterial3Api::class)

package io.github.liathy.musikpl.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.liathy.musikpl.core.Core
import uniffi.musikpl_mobile.LyricsKind

/** The playing song's lyrics; synced ones light up and follow the song. */
@Composable
fun LyricsSheet(onClose: () -> Unit) {
    DisposableEffect(Unit) {
        Core.lyricsVisible = true
        onDispose { Core.lyricsVisible = false }
    }
    val lyrics by Core.lyrics.collectAsStateWithLifecycle()
    val line by Core.lyricsLine.collectAsStateWithLifecycle()
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    ModalBottomSheet(onDismissRequest = onClose, sheetState = sheet, containerColor = Palette.background) {
        val state = lyrics
        val message = when (state?.kind) {
            null, LyricsKind.IDLE -> "Play a song to see its lyrics."
            LyricsKind.LOADING -> "Looking for the lyrics…"
            LyricsKind.MISSING -> "No lyrics found for this song."
            LyricsKind.INSTRUMENTAL -> "♪ Instrumental ♪"
            LyricsKind.FAILED -> state.error ?: "Couldn't load the lyrics."
            else -> null
        }
        if (message != null || state == null) {
            Box(Modifier.fillMaxWidth().padding(vertical = 120.dp), contentAlignment = Alignment.Center) {
                Text(message.orEmpty(), color = Palette.secondary, style = MaterialTheme.typography.titleMedium)
            }
            if (state?.kind == LyricsKind.FAILED) {
                Box(Modifier.fillMaxWidth().padding(bottom = 48.dp), contentAlignment = Alignment.Center) {
                    Pill("Try again", { Core.act { retryLyrics() } }, primary = false)
                }
            }
            return@ModalBottomSheet
        }
        val synced = state.kind == LyricsKind.SYNCED
        val list = rememberLazyListState()
        // Keep the line being sung near the top third.
        LaunchedEffect(line, synced) {
            val current = line ?: return@LaunchedEffect
            if (synced && !list.isScrollInProgress) {
                list.animateScrollToItem(current, scrollOffset = -list.layoutInfo.viewportSize.height / 3)
            }
        }
        LazyColumn(
            state = list,
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = 24.dp, end = 24.dp, top = 24.dp, bottom = 240.dp),
        ) {
            itemsIndexed(state.lines) { index, lyric ->
                val current = line
                val target = when {
                    !synced -> Palette.text
                    current == index -> Color.White
                    current != null && index < current -> Palette.secondary
                    else -> Palette.muted
                }
                val color by animateColorAsState(target, label = "lyric")
                Text(
                    text = if (synced && lyric.text.isEmpty()) "♪" else lyric.text,
                    color = color,
                    fontSize = if (synced) 26.sp else 18.sp,
                    lineHeight = if (synced) 32.sp else 26.sp,
                    fontWeight = if (synced) FontWeight.Bold else FontWeight.Normal,
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable(enabled = synced) { Core.act { seek(lyric.at) } }
                        .padding(vertical = if (synced) 10.dp else 2.dp),
                )
            }
        }
    }
}
