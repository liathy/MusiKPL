package io.github.liathy.musikpl.core

import android.content.Context
import android.util.Log
import com.yausername.youtubedl_android.YoutubeDL
import com.yausername.youtubedl_android.YoutubeDLRequest
import java.util.concurrent.TimeUnit
import okhttp3.OkHttpClient
import okhttp3.RequestBody.Companion.toRequestBody
import org.schabi.newpipe.extractor.InfoItem
import org.schabi.newpipe.extractor.NewPipe
import org.schabi.newpipe.extractor.ServiceList
import org.schabi.newpipe.extractor.downloader.Downloader
import org.schabi.newpipe.extractor.downloader.Request
import org.schabi.newpipe.extractor.downloader.Response
import org.schabi.newpipe.extractor.exceptions.ReCaptchaException
import org.schabi.newpipe.extractor.playlist.PlaylistInfo
import org.schabi.newpipe.extractor.services.youtube.linkHandler.YoutubeSearchQueryHandlerFactory
import org.schabi.newpipe.extractor.stream.DeliveryMethod
import org.schabi.newpipe.extractor.stream.StreamInfo
import org.schabi.newpipe.extractor.stream.StreamInfoItem
import uniffi.musikpl_mobile.AndroidExtractor
import uniffi.musikpl_mobile.MobileException
import uniffi.musikpl_mobile.Song

private const val TAG = "MusiKPL.Extractor"

/** NewPipe's network access, through OkHttp. */
class NewPipeDownloader(private val client: OkHttpClient) : Downloader() {
    override fun execute(request: Request): Response {
        val data = request.dataToSend()
        val body = when {
            data != null -> data.toRequestBody(null)
            request.httpMethod() == "POST" -> ByteArray(0).toRequestBody(null)
            else -> null
        }
        val builder = okhttp3.Request.Builder()
            .method(request.httpMethod(), body)
            .url(request.url())
            .header("User-Agent", USER_AGENT)
        for ((name, values) in request.headers()) {
            builder.removeHeader(name)
            values.forEach { builder.addHeader(name, it) }
        }
        client.newCall(builder.build()).execute().use { response ->
            if (response.code == 429) {
                throw ReCaptchaException("YouTube asked to confirm this isn't a bot", request.url())
            }
            return Response(
                response.code,
                response.message,
                response.headers.toMultimap(),
                response.body?.string(),
                response.request.url.toString(),
            )
        }
    }

    companion object {
        const val USER_AGENT =
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:140.0) Gecko/20100101 Firefox/140.0"
    }
}

/**
 * Finds songs and their audio: NewPipe first (fast, in-process), yt-dlp
 * (youtubedl-android) when NewPipe can't give a stream.
 *
 * Sources are the core's: `ytsearchN:query`, a Mix link
 * (`watch?v=ID&list=RDID`), or any other YouTube link.
 */
class YouTubeExtractor(private val context: Context) : AndroidExtractor {
    private val youtube = ServiceList.YouTube

    @Volatile
    private var ytDlpReady = false

    init {
        val client = OkHttpClient.Builder()
            .connectTimeout(15, TimeUnit.SECONDS)
            .readTimeout(30, TimeUnit.SECONDS)
            .build()
        NewPipe.init(NewPipeDownloader(client))
        // yt-dlp unpacks its Python on first use; get it ready meanwhile.
        Thread({ ytDlp() }, "musikpl-ytdlp-init").start()
    }

    private fun ytDlp(): YoutubeDL? {
        if (!ytDlpReady) {
            synchronized(this) {
                if (!ytDlpReady) {
                    ytDlpReady = runCatching { YoutubeDL.getInstance().init(context) }
                        .onFailure { Log.w(TAG, "yt-dlp isn't available", it) }
                        .isSuccess
                }
            }
        }
        return if (ytDlpReady) YoutubeDL.getInstance() else null
    }

    override fun list(source: String, limit: UInt): List<Song> = failing {
        val search = parseSearch(source)
        when {
            search != null -> searchVideos(search.second, minOf(search.first, limit.toInt()))
            source.contains("list=") -> playlist(source, limit.toInt())
            else -> listOf(video(source))
        }
    }

    override fun search(query: String): List<Song> = failing {
        val text = query.trim()
        if (text.startsWith("http://") || text.startsWith("https://")) {
            return@failing if (text.contains("list=")) playlist(text, 100) else listOf(video(text))
        }
        // YouTube Music's songs have clean titles and artists; plain
        // YouTube catches what isn't on YouTube Music.
        val songs = runCatching { searchWith(text, YoutubeSearchQueryHandlerFactory.MUSIC_SONGS, 20) }
            .getOrDefault(emptyList())
        songs.ifEmpty { searchVideos(text, 20) }
    }

    override fun streamUrl(song: Song): String = failing {
        val url = "https://www.youtube.com/watch?v=${song.id}"
        val fromNewPipe = runCatching {
            val info = StreamInfo.getInfo(youtube, url)
            info.audioStreams
                .filter { it.isUrl && it.deliveryMethod == DeliveryMethod.PROGRESSIVE_HTTP }
                .maxByOrNull { it.averageBitrate }
                ?.content
        }.onFailure { Log.w(TAG, "NewPipe couldn't get ${song.id}", it) }.getOrNull()
        fromNewPipe ?: streamFromYtDlp(url)
    }

    private fun streamFromYtDlp(url: String): String {
        val ytDlp = ytDlp() ?: throw IllegalStateException("YouTube didn't give this song's audio")
        val request = YoutubeDLRequest(url).apply {
            addOption("-f", "bestaudio")
            addOption("--get-url")
            addOption("--no-playlist")
            addOption("--no-warnings")
        }
        val out = ytDlp.execute(request).out
        return out.lineSequence().map { it.trim() }.firstOrNull { it.startsWith("http") }
            ?: throw IllegalStateException("YouTube didn't give this song's audio")
    }

    private fun searchVideos(query: String, count: Int): List<Song> =
        searchWith(query, YoutubeSearchQueryHandlerFactory.VIDEOS, count)

    private fun searchWith(query: String, filter: String, count: Int): List<Song> {
        val extractor = youtube.getSearchExtractor(query, listOf(filter), "")
        extractor.fetchPage()
        val songs = mutableListOf<Song>()
        var page = extractor.initialPage
        while (true) {
            songs += page.items.mapNotNull(::song)
            if (songs.size >= count || !page.hasNextPage()) break
            page = extractor.getPage(page.nextPage)
        }
        return songs.take(count)
    }

    private fun playlist(url: String, limit: Int): List<Song> {
        val info = PlaylistInfo.getInfo(youtube, url)
        return info.relatedItems.mapNotNull(::song).take(limit)
    }

    private fun video(url: String): Song {
        val info = StreamInfo.getInfo(youtube, url)
        return Song(
            id = info.id,
            title = info.name,
            artist = "",
            channel = info.uploaderName.orEmpty(),
            duration = info.duration.takeIf { it > 0 }?.toDouble(),
            cover = "",
        )
    }

    private fun song(item: InfoItem): Song? {
        if (item !is StreamInfoItem) return null
        val id = runCatching { youtube.streamLHFactory.getId(item.url) }.getOrNull() ?: return null
        return Song(
            id = id,
            title = item.name,
            artist = "",
            channel = item.uploaderName.orEmpty(),
            duration = item.duration.takeIf { it > 0 }?.toDouble(),
            cover = "",
        )
    }

    /** `ytsearchN:query` as (N, query). */
    private fun parseSearch(source: String): Pair<Int, String>? {
        if (!source.startsWith("ytsearch")) return null
        val rest = source.removePrefix("ytsearch")
        val colon = rest.indexOf(':').takeIf { it >= 0 } ?: return null
        val count = rest.substring(0, colon).ifEmpty { "1" }.toIntOrNull() ?: return null
        return count to rest.substring(colon + 1)
    }

    /** Run `block`, turning any failure into the engine's error. */
    private inline fun <T> failing(block: () -> T): T = try {
        block()
    } catch (error: Throwable) {
        throw MobileException.Failed(error.message ?: error.javaClass.simpleName)
    }
}
