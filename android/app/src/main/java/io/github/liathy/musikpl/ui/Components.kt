@file:OptIn(ExperimentalFoundationApi::class)

package io.github.liathy.musikpl.ui

import androidx.annotation.DrawableRes
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.basicMarquee
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import kotlinx.coroutines.launch
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import coil3.compose.AsyncImage
import io.github.liathy.musikpl.R
import uniffi.musikpl_mobile.Song

/** One of the app's own icons (src/assets/icons). */
@Composable
fun AppIcon(
    @DrawableRes icon: Int,
    description: String?,
    modifier: Modifier = Modifier,
    tint: Color = Palette.icon,
    size: Dp = 22.dp,
) {
    Icon(painterResource(icon), description, modifier.size(size), tint = tint)
}

@Composable
fun IconAction(
    @DrawableRes icon: Int,
    description: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    tint: Color = Palette.icon,
    size: Dp = 22.dp,
) {
    IconButton(onClick = onClick, modifier = modifier) {
        AppIcon(icon, description, tint = tint, size = size)
    }
}

/**
 * One line of text that scrolls back and forth when it doesn't fit, like
 * the desktop app (no "…").
 */
@Composable
fun ScrollingText(
    text: String,
    style: TextStyle,
    modifier: Modifier = Modifier,
    color: Color = Color.Unspecified,
) {
    Text(
        text = text,
        style = style,
        color = color,
        maxLines = 1,
        softWrap = false,
        modifier = modifier.basicMarquee(iterations = Int.MAX_VALUE, initialDelayMillis = 2000),
    )
}

/** A square, rounded cover: the image, or a music note until it loads. */
@Composable
fun Cover(url: String?, modifier: Modifier = Modifier, corner: Dp = 8.dp) {
    Box(
        modifier
            .clip(RoundedCornerShape(corner))
            .background(Palette.surface),
        contentAlignment = Alignment.Center,
    ) {
        AppIcon(R.drawable.ic_music, null, tint = Palette.muted, size = 20.dp)
        if (!url.isNullOrEmpty()) {
            AsyncImage(
                model = url,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.matchParentSize(),
            )
        }
    }
}

/** A round photo (artists, people), or the name's initial. */
@Composable
fun RoundPhoto(url: String?, name: String, modifier: Modifier = Modifier) {
    Box(
        modifier.clip(CircleShape).background(Palette.surface),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            name.trim().take(1).uppercase(),
            style = MaterialTheme.typography.titleLarge,
            color = Palette.secondary,
        )
        if (!url.isNullOrEmpty()) {
            AsyncImage(
                model = url,
                contentDescription = name,
                contentScale = ContentScale.Crop,
                modifier = Modifier.matchParentSize(),
            )
        }
    }
}

/** A song as a cover card (Home rows). */
@Composable
fun SongCard(song: Song, onPlay: () -> Unit, onMore: () -> Unit, liked: Boolean = false) {
    Column(
        Modifier
            .width(148.dp)
            .clip(RoundedCornerShape(10.dp))
            .combinedClickable(onClick = onPlay, onLongClick = onMore),
    ) {
        Box {
            Cover(song.cover, Modifier.size(148.dp), corner = 10.dp)
            if (liked) {
                Box(
                    Modifier
                        .padding(6.dp)
                        .size(26.dp)
                        .clip(CircleShape)
                        .background(Color.Black.copy(alpha = 0.7f))
                        .align(Alignment.TopEnd),
                    contentAlignment = Alignment.Center,
                ) {
                    AppIcon(R.drawable.ic_heart_filled, "Liked", tint = Palette.like, size = 14.dp)
                }
            }
            Box(
                Modifier
                    .padding(6.dp)
                    .size(30.dp)
                    .clip(CircleShape)
                    .background(Color.Black.copy(alpha = 0.7f))
                    .align(Alignment.BottomEnd)
                    .combinedClickable(onClick = onMore),
                contentAlignment = Alignment.Center,
            ) {
                AppIcon(R.drawable.ic_add, "More", size = 16.dp)
            }
        }
        Spacer(Modifier.size(6.dp))
        ScrollingText(song.title, MaterialTheme.typography.titleMedium)
        ScrollingText(song.artist, MaterialTheme.typography.bodySmall, color = Palette.secondary)
    }
}

/** A song as a list row: cover, title, artist, and a + for its menu. */
@Composable
fun SongRow(
    song: Song,
    onPlay: () -> Unit,
    onMore: () -> Unit,
    modifier: Modifier = Modifier,
    playing: Boolean = false,
    liked: Boolean = false,
    leading: String? = null,
) {
    Row(
        modifier
            .fillMaxWidth()
            .combinedClickable(onClick = onPlay, onLongClick = onMore)
            .padding(horizontal = 16.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (leading != null) {
            Text(
                leading,
                style = MaterialTheme.typography.bodyMedium,
                color = Palette.muted,
                textAlign = TextAlign.Center,
                modifier = Modifier.width(28.dp),
            )
        }
        Cover(song.cover, Modifier.size(52.dp))
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            ScrollingText(
                song.title,
                MaterialTheme.typography.titleMedium,
                color = if (playing) Palette.like else Palette.text,
            )
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (liked) {
                    AppIcon(R.drawable.ic_heart_filled, "Liked", tint = Palette.like, size = 12.dp)
                    Spacer(Modifier.width(4.dp))
                }
                val detail = listOfNotNull(song.artist.ifEmpty { null }, song.duration?.let(::clock))
                ScrollingText(
                    detail.joinToString(" · "),
                    MaterialTheme.typography.bodySmall,
                    color = Palette.secondary,
                )
            }
        }
        IconAction(R.drawable.ic_add, "More", onMore)
    }
}

/** A row for a Last.fm song (artist pages, profiles), found on YouTube when played. */
@Composable
fun LastFmRow(
    title: String,
    detail: String,
    onPlay: () -> Unit,
    onMore: () -> Unit,
    leading: String? = null,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .combinedClickable(onClick = onPlay, onLongClick = onMore)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (leading != null) {
            Text(
                leading,
                style = MaterialTheme.typography.bodyMedium,
                color = Palette.muted,
                textAlign = TextAlign.Center,
                modifier = Modifier.width(28.dp),
            )
        }
        Column(Modifier.weight(1f)) {
            ScrollingText(title, MaterialTheme.typography.titleMedium)
            if (detail.isNotEmpty()) {
                ScrollingText(detail, MaterialTheme.typography.bodySmall, color = Palette.secondary)
            }
        }
        IconAction(R.drawable.ic_add, "More", onMore)
    }
}

@Composable
fun SectionTitle(text: String, modifier: Modifier = Modifier, trailing: @Composable () -> Unit = {}) {
    Row(
        modifier
            .fillMaxWidth()
            .padding(start = 16.dp, end = 4.dp, top = 20.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(text, style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
        trailing()
    }
}

/** A pill-shaped button like the desktop's. */
@Composable
fun Pill(text: String, onClick: () -> Unit, primary: Boolean = true, modifier: Modifier = Modifier) {
    Box(
        modifier
            .clip(RoundedCornerShape(50))
            .background(if (primary) Color.White else Palette.surface)
            .combinedClickable(onClick = onClick)
            .padding(horizontal = 18.dp, vertical = 8.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text,
            style = MaterialTheme.typography.labelLarge,
            color = if (primary) Color.Black else Palette.text,
        )
    }
}

fun clock(seconds: Double): String {
    val total = seconds.toLong().coerceAtLeast(0)
    return "%d:%02d".format(total / 60, total % 60)
}

/** "1.2K", "3.4M": listener and play counts. */
fun compact(count: ULong): String {
    val n = count.toDouble()
    return when {
        n >= 1e9 -> "%.1fB".format(n / 1e9)
        n >= 1e6 -> "%.1fM".format(n / 1e6)
        n >= 1e3 -> "%.1fK".format(n / 1e3)
        else -> count.toString()
    }.replace(".0", "")
}

/**
 * Swipe the song's cover sideways: left plays the next song, right the
 * previous one. The cover follows the finger and springs back.
 */
@Composable
fun Modifier.swipeSongs(enabled: Boolean = true): Modifier {
    val offset = remember { androidx.compose.animation.core.Animatable(0f) }
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    val threshold = with(androidx.compose.ui.platform.LocalDensity.current) { 72.dp.toPx() }
    return this
        .graphicsLayer {
            translationX = offset.value
            alpha = 1f - (kotlin.math.abs(offset.value) / (threshold * 6)).coerceAtMost(0.4f)
        }
        .pointerInput(enabled) {
            if (!enabled) return@pointerInput
            detectHorizontalDragGestures(
                onDragEnd = {
                    val moved = offset.value
                    when {
                        moved < -threshold -> io.github.liathy.musikpl.core.Core.act { next() }
                        moved > threshold -> io.github.liathy.musikpl.core.Core.act { previous() }
                    }
                    scope.launch { offset.animateTo(0f) }
                },
                onDragCancel = { scope.launch { offset.animateTo(0f) } },
            ) { change, amount ->
                change.consume()
                scope.launch { offset.snapTo(offset.value + amount) }
            }
        }
}
