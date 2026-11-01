package io.github.liathy.musikpl.playback

import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.annotation.OptIn
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import io.github.liathy.musikpl.core.Core
import io.github.liathy.musikpl.core.NewPipeDownloader
import uniffi.musikpl_mobile.AndroidPlayer
import uniffi.musikpl_mobile.Song

private const val TAG = "MusiKPL.Audio"

/**
 * ExoPlayer as the engine's player.
 *
 * The engine calls in from its own thread; every call is handed to the
 * player's (main) thread. The position is sampled there a few times a
 * second, so the engine never waits on the main thread.
 */
@OptIn(UnstableApi::class)
class ExoAudio(context: Context) : AndroidPlayer {
    private val main = Handler(Looper.getMainLooper())

    val exo: ExoPlayer = ExoPlayer.Builder(context)
        .setAudioAttributes(
            AudioAttributes.Builder()
                .setUsage(C.USAGE_MEDIA)
                .setContentType(C.AUDIO_CONTENT_TYPE_MUSIC)
                .build(),
            /* handleAudioFocus = */ true,
        )
        // Pause when headphones are unplugged.
        .setHandleAudioBecomingNoisy(true)
        .setWakeMode(C.WAKE_MODE_NETWORK)
        .setMediaSourceFactory(
            DefaultMediaSourceFactory(
                DefaultHttpDataSource.Factory()
                    .setUserAgent(NewPipeDownloader.USER_AGENT)
                    .setAllowCrossProtocolRedirects(true),
            ),
        )
        .build()

    @Volatile
    private var finished = false

    @Volatile
    private var position: Double? = null

    /** The song the engine shows, for the notification and lock screen. */
    @Volatile
    private var shown: Song? = null

    init {
        exo.addListener(object : Player.Listener {
            override fun onPlaybackStateChanged(state: Int) {
                if (state == Player.STATE_ENDED) {
                    finished = true
                    Core.wake()
                }
            }

            override fun onPlayerError(error: PlaybackException) {
                Log.w(TAG, "Playback failed: ${error.errorCodeName}", error)
                finished = true
                Core.wake()
            }

            override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
                // The system paused or resumed (a call, headphones): tell
                // the engine, which shows and syncs the pause.
                val fromSystem = reason == Player.PLAY_WHEN_READY_CHANGE_REASON_AUDIO_FOCUS_LOSS ||
                    reason == Player.PLAY_WHEN_READY_CHANGE_REASON_AUDIO_BECOMING_NOISY
                if (fromSystem) Core.act { setPaused(!playWhenReady) }
            }
        })
        main.post(object : Runnable {
            override fun run() {
                val ready = exo.playbackState == Player.STATE_READY ||
                    exo.playbackState == Player.STATE_BUFFERING
                position = if (ready && exo.currentMediaItem != null) exo.currentPosition / 1000.0 else null
                main.postDelayed(this, if (exo.isPlaying) 200 else 500)
            }
        })
    }

    private fun metadata(song: Song?): MediaMetadata = MediaMetadata.Builder()
        .setTitle(song?.title)
        .setArtist(song?.artist)
        .setArtworkUri(song?.cover?.let(Uri::parse))
        .build()

    /** The engine's shown song changed: update the notification. */
    fun showing(song: Song?) {
        if (song?.id == shown?.id) return
        shown = song
        main.post {
            val item = exo.currentMediaItem ?: return@post
            exo.replaceMediaItem(0, item.buildUpon().setMediaMetadata(metadata(song)).build())
        }
    }

    override fun play(url: String, volume: UByte, startSecs: Double, paused: Boolean): String? {
        val song = shown
        main.post {
            finished = false
            position = null
            exo.volume = volume.toFloat() / 100f
            exo.setMediaItem(
                MediaItem.Builder().setUri(url).setMediaMetadata(metadata(song)).build(),
                (startSecs * 1000).toLong(),
            )
            exo.playWhenReady = !paused
            exo.prepare()
        }
        return null
    }

    override fun setPaused(paused: Boolean) {
        main.post { exo.playWhenReady = !paused }
    }

    override fun seek(position: Double) {
        main.post { exo.seekTo((position * 1000).toLong()) }
    }

    override fun setSpeed(speed: Double) {
        main.post { exo.setPlaybackSpeed(speed.toFloat()) }
    }

    override fun setVolume(volume: UByte) {
        main.post { exo.volume = volume.toFloat() / 100f }
    }

    override fun stop() {
        position = null
        main.post {
            exo.stop()
            exo.clearMediaItems()
        }
    }

    override fun position(): Double? = position

    override fun isFinished(): Boolean = finished
}
