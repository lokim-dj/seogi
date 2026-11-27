package io.github.lokimdj.seogi.ui

import android.content.Context
import android.media.MediaPlayer
import android.net.Uri

/**
 * Plays one meeting's recording, read only: the file (on this phone, or the
 * copy downloaded from Drive) is opened for reading and never changed.
 */
class AudioPlayer(private val context: Context) {
    private var player: MediaPlayer? = null
    private var source: Uri? = null

    val playing: Boolean get() = runCatching { player?.isPlaying == true }.getOrDefault(false)
    val positionMs: Long get() = runCatching { player?.currentPosition?.toLong() ?: 0L }.getOrDefault(0L)
    val durationMs: Long get() = runCatching { player?.duration?.toLong()?.coerceAtLeast(0) ?: 0L }.getOrDefault(0L)

    /** Plays [uri] from [fromMs] (or from where it was); false when it cannot be opened. */
    fun play(uri: Uri, fromMs: Long?): Boolean {
        try {
            if (source != uri || player == null) {
                release()
                player = MediaPlayer().apply {
                    setDataSource(context, uri)
                    prepare()
                }
                source = uri
            }
            val p = player ?: return false
            if (fromMs != null) p.seekTo(fromMs.coerceAtLeast(0).toInt())
            p.start()
            return true
        } catch (e: Exception) {
            release()
            return false
        }
    }

    fun pause() {
        runCatching { player?.pause() }
    }

    fun release() {
        runCatching { player?.release() }
        player = null
        source = null
    }
}
