package io.github.lokimdj.seogi.record

import android.content.ContentValues
import android.content.Context
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaMuxer
import android.os.Environment
import android.provider.MediaStore
import io.github.lokimdj.seogi.data.FileNames
import io.github.lokimdj.seogi.data.Recording
import java.io.File
import java.nio.ByteBuffer
import java.security.MessageDigest
import java.time.Instant
import java.time.ZoneId

/**
 * Turns a finished (or cut off) AAC stream into the recording that stays on
 * the phone: the same AAC frames moved, not re-encoded, from the ADTS stream
 * into an .m4a, written to MediaStore `Recordings/서기/`. If moving fails, the
 * ADTS stream itself becomes the recording (.aac), so nothing is lost. After
 * this the file is never changed by the app.
 */
object Finalizer {
    const val FOLDER = "서기"

    fun finalize(context: Context, stream: File, state: SessionState, recovered: Boolean): Recording {
        val zone = ZoneId.of(state.zone)
        val start = Instant.ofEpochMilli(state.startedAt).atZone(zone)
        val base = FileNames.baseName(start, state.eventTitle)
        val m4a = File(context.cacheDir, "${state.id}.m4a")
        val (source, ext, mime, durationUs) = try {
            Moved(m4a, "m4a", "audio/mp4", remux(stream, m4a))
        } catch (e: Exception) {
            m4a.delete()
            Moved(stream, "aac", "audio/aac", 0L)
        }
        try {
            val (uri, name, sha) = writeToMediaStore(context, source, "$base.$ext", mime)
            val durationMs = if (durationUs > 0) durationUs / 1000 else estimateMs(stream)
            return Recording(
                id = state.id,
                title = FileNames.displayTitle(start, state.eventTitle),
                eventTitle = state.eventTitle,
                startedAt = state.startedAt,
                zone = state.zone,
                durationMs = durationMs,
                marksMs = state.marksMs.filter { it <= durationMs || durationUs == 0L },
                fileName = name,
                mimeType = mime,
                mediaUri = uri,
                sizeBytes = source.length(),
                sha256 = sha,
                recovered = recovered,
            )
        } finally {
            m4a.delete()
        }
    }

    private data class Moved(val file: File, val ext: String, val mime: String, val durationUs: Long)

    /** ADTS AAC → MP4 container, sample for sample. Returns the last sample's time in µs. */
    private fun remux(stream: File, out: File): Long {
        val ex = MediaExtractor()
        var muxer: MediaMuxer? = null
        try {
            ex.setDataSource(stream.path)
            require(ex.trackCount > 0) { "no track" }
            ex.selectTrack(0)
            muxer = MediaMuxer(out.path, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
            val track = muxer.addTrack(ex.getTrackFormat(0))
            muxer.start()
            val buf = ByteBuffer.allocate(256 * 1024)
            val info = MediaCodec.BufferInfo()
            var last = 0L
            var count = 0
            while (true) {
                val size = ex.readSampleData(buf, 0)
                if (size < 0) break
                info.set(0, size, ex.sampleTime, if (ex.sampleFlags and MediaExtractor.SAMPLE_FLAG_SYNC != 0) MediaCodec.BUFFER_FLAG_KEY_FRAME else 0)
                muxer.writeSampleData(track, buf, info)
                last = ex.sampleTime
                count++
                ex.advance()
            }
            require(count > 0) { "empty stream" }
            muxer.stop()
            return last
        } finally {
            try { muxer?.release() } catch (_: Exception) {}
            ex.release()
        }
    }

    /** 64 kbps → 8000 bytes a second; only used when the container gave no time. */
    private fun estimateMs(stream: File) = stream.length() * 1000 / 8000

    private data class Saved(val uri: String, val name: String, val sha256: String)

    private fun writeToMediaStore(context: Context, file: File, displayName: String, mime: String): Saved {
        val resolver = context.contentResolver
        val collection = MediaStore.Audio.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, displayName)
            put(MediaStore.MediaColumns.MIME_TYPE, mime)
            put(MediaStore.MediaColumns.RELATIVE_PATH, "${Environment.DIRECTORY_RECORDINGS}/$FOLDER")
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        val uri = resolver.insert(collection, values) ?: error("MediaStore insert failed")
        try {
            val digest = MessageDigest.getInstance("SHA-256")
            var written = 0L
            resolver.openOutputStream(uri, "w")!!.use { out ->
                file.inputStream().use { input ->
                    val buf = ByteArray(64 * 1024)
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        digest.update(buf, 0, n)
                        out.write(buf, 0, n)
                        written += n
                    }
                }
            }
            check(written == file.length()) { "short write" }
            resolver.update(uri, ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null)
            val name = resolver.query(uri, arrayOf(MediaStore.MediaColumns.DISPLAY_NAME), null, null, null)?.use {
                if (it.moveToFirst()) it.getString(0) else null
            } ?: displayName
            return Saved(uri.toString(), name, digest.digest().joinToString("") { "%02x".format(it) })
        } catch (e: Exception) {
            // Only our own half-written pending copy; the stream stays for the next try.
            resolver.delete(uri, null, null)
            throw e
        }
    }
}
