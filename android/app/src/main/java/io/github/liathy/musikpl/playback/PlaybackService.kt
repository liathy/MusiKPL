package io.github.liathy.musikpl.playback

import android.app.PendingIntent
import android.content.Intent
import androidx.annotation.OptIn
import androidx.media3.common.ForwardingPlayer
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.DefaultMediaNotificationProvider
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import io.github.liathy.musikpl.MainActivity
import io.github.liathy.musikpl.R
import io.github.liathy.musikpl.core.Core

/**
 * Keeps MusiKPL playing in the background, with the media notification,
 * lock screen and headset controls.
 *
 * The session's play, pause, next and previous go to the engine, which
 * owns the queue and Auto Mix; ExoPlayer only plays what it's given.
 */
@OptIn(UnstableApi::class)
class PlaybackService : MediaSessionService() {
    private var session: MediaSession? = null

    override fun onCreate() {
        super.onCreate()
        setMediaNotificationProvider(
            DefaultMediaNotificationProvider.Builder(this).build().apply {
                setSmallIcon(R.drawable.ic_notification)
            },
        )
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        session = MediaSession.Builder(this, EnginePlayer(Core.audio.exo))
            .setSessionActivity(open)
            .build()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

    override fun onTaskRemoved(rootIntent: Intent?) {
        // Swiped away while nothing plays: save and go.
        if (!Core.audio.exo.playWhenReady) {
            Core.save()
            stopSelf()
        }
    }

    override fun onDestroy() {
        session?.release()
        session = null
        super.onDestroy()
    }
}

/** ExoPlayer, with transport controls routed to the engine. */
@OptIn(UnstableApi::class)
private class EnginePlayer(player: Player) : ForwardingPlayer(player) {
    private val transport = listOf(
        COMMAND_SEEK_TO_NEXT,
        COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
        COMMAND_SEEK_TO_PREVIOUS,
        COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
    )

    override fun getAvailableCommands(): Player.Commands =
        super.getAvailableCommands().buildUpon().addAll(*transport.toIntArray()).build()

    override fun isCommandAvailable(command: Int): Boolean =
        command in transport || super.isCommandAvailable(command)

    override fun play() = Core.act { setPaused(false) }

    override fun pause() = Core.act { setPaused(true) }

    override fun seekToNext() = Core.act { next() }

    override fun seekToNextMediaItem() = Core.act { next() }

    override fun seekToPrevious() = Core.act { previous() }

    override fun seekToPreviousMediaItem() = Core.act { previous() }

    override fun seekTo(positionMs: Long) = Core.act { seek(positionMs / 1000.0) }
}
