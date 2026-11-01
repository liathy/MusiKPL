package io.github.liathy.musikpl.playback

import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import androidx.annotation.OptIn
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import io.github.liathy.musikpl.core.Core
import io.github.liathy.musikpl.core.NewPipeDownloader
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.sin
import uniffi.musikpl_mobile.AndroidPlayer
import uniffi.musikpl_mobile.FadeShape
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
class ExoAudio(private val context: Context) : AndroidPlayer {
    private val main = Handler(Looper.getMainLooper())

    /** The player the notification follows. */
    val exo: ExoPlayer = newPlayer(main = true)

    private fun newPlayer(main: Boolean): ExoPlayer = ExoPlayer.Builder(context)
        .setAudioAttributes(
            AudioAttributes.Builder()
                .setUsage(C.USAGE_MEDIA)
                .setContentType(C.AUDIO_CONTENT_TYPE_MUSIC)
                .build(),
            // Only the main player asks for audio focus (the other is a
            // song's last seconds under a crossfade).
            /* handleAudioFocus = */ main,
        )
        // Pause when headphones are unplugged.
        .setHandleAudioBecomingNoisy(main)
        .setWakeMode(C.WAKE_MODE_NETWORK)
        .setMediaSourceFactory(
            // Streams over HTTP, and downloaded songs from their files.
            DefaultMediaSourceFactory(
                DefaultDataSource.Factory(
                    context,
                    DefaultHttpDataSource.Factory()
                        .setUserAgent(NewPipeDownloader.USER_AGENT)
                        .setAllowCrossProtocolRedirects(true),
                ),
            ),
        )
        .build()

    /** The volume the engine set (0 to 1); fades go up to it. */
    @Volatile
    private var level = 1f

    /** The song fading out under a crossfade. */
    private var tail: ExoPlayer? = null
    private var ramp: Runnable? = null
    private var pendingSwitch: Runnable? = null

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
        level = volume.toFloat() / 100f
        main.post {
            endFade()
            start(url, startSecs, paused, song, level)
        }
        return null
    }

    private fun start(url: String, startSecs: Double, paused: Boolean, song: Song?, volume: Float) {
        finished = false
        position = null
        exo.volume = volume
        val uri = if (url.startsWith("/")) Uri.fromFile(java.io.File(url)) else Uri.parse(url)
        exo.setMediaItem(
            MediaItem.Builder().setUri(uri).setMediaMetadata(metadata(song)).build(),
            (startSecs * 1000).toLong(),
        )
        exo.playWhenReady = !paused
        exo.prepare()
    }

    /**
     * Crossfade: a second player picks up the playing song a moment ahead
     * and takes over exactly there, while this one starts the next song;
     * then the volumes cross along the curve.
     */
    override fun crossfade(url: String, volume: UByte, startSecs: Double, seconds: Double, curve: FadeShape): String? {
        val song = shown
        level = volume.toFloat() / 100f
        main.post {
            endFade()
            val item = exo.currentMediaItem
            if (item == null || !exo.isPlaying) {
                start(url, startSecs, false, song, level)
                return@post
            }
            val lead = 700L
            val from = exo.currentPosition + lead
            val old = newPlayer(main = false)
            old.volume = level
            old.setMediaItem(item, from)
            old.playWhenReady = false
            old.prepare()
            tail = old
            var switched = false
            val switch = Runnable {
                if (switched) return@Runnable
                switched = true
                pendingSwitch = null
                if (old.playbackState == Player.STATE_READY) old.play() else endFade()
                start(url, startSecs, false, song, if (tail != null) 0f else level)
                if (tail != null) startRamp(seconds, curve)
            }
            old.addListener(object : Player.Listener {
                override fun onPlaybackStateChanged(state: Int) {
                    if (state == Player.STATE_READY && !switched) {
                        main.postDelayed(switch, (from - exo.currentPosition).coerceAtLeast(0))
                    }
                }

                override fun onPlayerError(error: PlaybackException) {
                    tail?.release()
                    tail = null
                    switch.run()
                }
            })
            // Not ready in time: just switch.
            pendingSwitch = switch
            main.postDelayed(switch, lead + 1200)
        }
        return null
    }

    private fun startRamp(seconds: Double, curve: FadeShape) {
        val asked = SystemClock.uptimeMillis()
        var started = -1L
        val length = (seconds * 1000).coerceAtLeast(100.0)
        val step = object : Runnable {
            override fun run() {
                val now = SystemClock.uptimeMillis()
                // The fade starts when the new song is heard.
                if (started < 0) {
                    if (!exo.isPlaying && now - asked < 5000) {
                        main.postDelayed(this, 40)
                        return
                    }
                    started = now
                }
                val t = ((now - started) / length).coerceIn(0.0, 1.0)
                exo.volume = level * gain(curve, t)
                tail?.volume = level * gain(curve, 1 - t)
                if (t >= 1.0) endFade() else main.postDelayed(this, 40)
            }
        }
        ramp = step
        main.post(step)
    }

    /** Stop the song fading out, and put the volume back. */
    private fun endFade() {
        pendingSwitch?.let(main::removeCallbacks)
        pendingSwitch = null
        ramp?.let(main::removeCallbacks)
        ramp = null
        tail?.release()
        tail = null
        exo.volume = level
    }

    private fun gain(curve: FadeShape, t: Double): Float = when (curve) {
        FadeShape.LINEAR -> t
        FadeShape.SMOOTH -> (1 - cos(PI * t)) / 2
        FadeShape.EQUAL_POWER -> sin(PI / 2 * t)
        FadeShape.EXPONENTIAL -> t * t
    }.toFloat()

    override fun setPaused(paused: Boolean) {
        main.post {
            if (paused) endFade()
            exo.playWhenReady = !paused
        }
    }

    override fun seek(position: Double) {
        main.post { exo.seekTo((position * 1000).toLong()) }
    }

    override fun setSpeed(speed: Double) {
        main.post { exo.setPlaybackSpeed(speed.toFloat()) }
    }

    override fun setVolume(volume: UByte) {
        level = volume.toFloat() / 100f
        // A fade in progress picks the new level up on its next step.
        main.post { if (ramp == null) exo.volume = level }
    }

    override fun stop() {
        position = null
        main.post {
            endFade()
            exo.stop()
            exo.clearMediaItems()
        }
    }

    override fun position(): Double? = position

    override fun isFinished(): Boolean = finished
}
