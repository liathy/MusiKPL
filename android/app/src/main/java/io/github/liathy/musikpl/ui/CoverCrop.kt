package io.github.liathy.musikpl.ui

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.net.Uri
import android.os.Build
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import java.io.File
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/** Covers are saved at this size, like the desktop. */
private const val COVER_SIZE = 512

/**
 * Read a picked photo (any format Android reads, HEIC included), no larger
 * than 2048 pixels.
 */
fun readPhoto(context: Context, uri: Uri): Bitmap? = runCatching {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        val source = ImageDecoder.createSource(context.contentResolver, uri)
        ImageDecoder.decodeBitmap(source) { decoder, info, _ ->
            val longest = max(info.size.width, info.size.height)
            if (longest > 2048) {
                val scale = 2048f / longest
                decoder.setTargetSize((info.size.width * scale).toInt(), (info.size.height * scale).toInt())
            }
            decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
        }
    } else {
        context.contentResolver.openInputStream(uri)!!.use { BitmapFactory.decodeStream(it) }
    }
}.getOrNull()

/** Save the square cut from `photo` as a JPEG the core keeps as the cover. */
fun saveSquare(context: Context, square: Bitmap): File {
    val small = Bitmap.createScaledBitmap(square, COVER_SIZE, COVER_SIZE, true)
    val file = File(context.cacheDir, "cover-${System.currentTimeMillis()}.jpg")
    file.outputStream().use { small.compress(Bitmap.CompressFormat.JPEG, 92, it) }
    return file
}

/**
 * Move and zoom (pinch) the photo inside the square it will fill, then
 * Save: `onDone` gets that square.
 */
@Composable
fun CoverCropDialog(photo: Bitmap, onDone: (Bitmap) -> Unit, onCancel: () -> Unit) {
    val image = remember(photo) { photo.asImageBitmap() }
    val frameDp = 300.dp
    val frame = with(LocalDensity.current) { frameDp.toPx() }
    val w = photo.width.toFloat()
    val h = photo.height.toFloat()
    // Zoom 1: the square spans the photo's shorter side.
    val base = frame / min(w, h)
    var zoom by remember { mutableFloatStateOf(1f) }
    // Where the photo's center sits, relative to the square's center.
    var offset by remember { mutableStateOf(Offset.Zero) }

    fun clamp(o: Offset, z: Float): Offset {
        val scale = base * z
        val maxX = (w * scale - frame) / 2f
        val maxY = (h * scale - frame) / 2f
        return Offset(o.x.coerceIn(-maxX, maxX), o.y.coerceIn(-maxY, maxY))
    }

    Dialog(onDismissRequest = onCancel, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Column(
            Modifier.fillMaxSize().background(Color.Black).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Text("Move and pinch to fit the square", style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.size(20.dp))
            Box(
                Modifier
                    .size(frameDp)
                    .clipToBounds()
                    .border(2.dp, Color.White)
                    .pointerInput(Unit) {
                        detectTransformGestures { _, pan, gestureZoom, _ ->
                            zoom = (zoom * gestureZoom).coerceIn(1f, 8f)
                            offset = clamp(offset + pan, zoom)
                        }
                    },
            ) {
                Canvas(Modifier.fillMaxSize()) {
                    val scale = base * zoom
                    val left = frame / 2f - w * scale / 2f + offset.x
                    val top = frame / 2f - h * scale / 2f + offset.y
                    drawImage(
                        image,
                        dstOffset = IntOffset(left.roundToInt(), top.roundToInt()),
                        dstSize = IntSize((w * scale).roundToInt(), (h * scale).roundToInt()),
                    )
                }
            }
            Spacer(Modifier.size(24.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Pill("Save", {
                    val scale = base * zoom
                    val side = (frame / scale).roundToInt().coerceIn(1, min(photo.width, photo.height))
                    val x = (w / 2f - (frame / 2f + offset.x) / scale).roundToInt().coerceIn(0, photo.width - side)
                    val y = (h / 2f - (frame / 2f + offset.y) / scale).roundToInt().coerceIn(0, photo.height - side)
                    onDone(Bitmap.createBitmap(photo, x, y, side, side))
                })
                Pill("Cancel", onCancel, primary = false)
            }
            Spacer(Modifier.size(12.dp))
            Text("Pinch to zoom", style = MaterialTheme.typography.bodySmall, color = Palette.secondary, modifier = Modifier.fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.Center)
        }
    }
}
