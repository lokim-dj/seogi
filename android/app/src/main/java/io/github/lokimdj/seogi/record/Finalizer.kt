package io.github.lokimdj.seogi.record

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaMuxer
import android.os.Environment
import android.provider.MediaStore
import io.github.lokimdj.seogi.data.FileNames
import io.github.lokimdj.seogi.data.Recording
import java.io.File
import java.io.FileDescriptor
import java.io.FileOutputStream
import java.nio.ByteBuffer
import java.security.MessageDigest
import java.time.Instant
import java.time.ZoneId

/**
 * Turns a finished (or cut off) AAC stream into the recording that stays on
 * the phone: the same AAC frames moved, not re-encoded, from the ADTS stream
 * into an .m4a, written straight into MediaStore `Recordings/서기/`. If moving fails, the
 * ADTS stream itself becomes the recording (.aac), so nothing is lost. After
 * this the file is never changed by the app.
 */
object Finalizer {
    const val FOLDER = "서기"

    /**
     * [onPending] gets the MediaStore address as soon as the item exists, so
     * a crash before the recording is listed can be cleaned up instead of
     * saved twice.
     */
    fun finalize(context: Context, stream: File, state: SessionState, recovered: Boolean, onPending: (String) -> Unit): Recording {
        val zone = ZoneId.of(state.zone)
        val start = Instant.ofEpochMilli(state.startedAt).atZone(zone)
        val base = FileNames.baseName(start, state.eventTitle)
        // The .m4a is written straight into its MediaStore item: the stream
        // plus one copy, not two.
        val saved = try {
            publish(context, "$base.m4a", "audio/mp4", onPending) { pfd -> remux(stream, pfd.fileDescriptor) }
        } catch (e: Exception) {
            publish(context, "$base.aac", "audio/aac", onPending) { pfd ->
                FileOutputStream(pfd.fileDescriptor).use { out -> stream.inputStream().use { it.copyTo(out, 64 * 1024) } }
                0L
            }
        }
        val durationMs = if (saved.durationUs > 0) saved.durationUs / 1000 else estimateMs(stream)
        return Recording(
            id = state.id,
            title = FileNames.displayTitle(start, state.eventTitle),
            eventTitle = state.eventTitle,
            startedAt = state.startedAt,
            zone = state.zone,
            durationMs = durationMs,
            marksMs = state.marksMs.filter { it <= durationMs || saved.durationUs == 0L },
            fileName = saved.name,
            mimeType = saved.mime,
            mediaUri = saved.uri,
            sizeBytes = saved.size,
            sha256 = saved.sha256,
            recovered = recovered,
        )
    }

    /** Removes an item a crash left behind before it was listed. */
    fun discard(context: Context, uri: String) {
        try {
            context.contentResolver.delete(Uri.parse(uri), null, null)
        } catch (_: Exception) {}
    }

    private data class Published(val uri: String, val name: String, val mime: String, val sha256: String, val size: Long, val durationUs: Long)

    private fun publish(
        context: Context,
        displayName: String,
        mime: String,
        onPending: (String) -> Unit,
        write: (ParcelFileDescriptor) -> Long,
    ): Published {
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
            onPending(uri.toString())
            val durationUs = resolver.openFileDescriptor(uri, "rw")!!.use(write)
            val digest = MessageDigest.getInstance("SHA-256")
            var size = 0L
            resolver.openInputStream(uri)!!.use { input ->
                val buf = ByteArray(64 * 1024)
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    digest.update(buf, 0, n)
                    size += n
                }
            }
            check(size > 0) { "empty file" }
            resolver.update(uri, ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null)
            val name = resolver.query(uri, arrayOf(MediaStore.MediaColumns.DISPLAY_NAME), null, null, null)?.use {
                if (it.moveToFirst()) it.getString(0) else null
            } ?: displayName
            return Published(uri.toString(), name, mime, digest.digest().joinToString("") { "%02x".format(it) }, size, durationUs)
        } catch (e: Exception) {
            // Only our own half-written pending copy; the stream stays for the next try.
            resolver.delete(uri, null, null)
            throw e
        }
    }

    /** ADTS AAC → MP4 container, sample for sample. Returns the last sample's time in µs. */
    private fun remux(stream: File, out: FileDescriptor): Long {
        val ex = MediaExtractor()
        var muxer: MediaMuxer? = null
        try {
            ex.setDataSource(stream.path)
            require(ex.trackCount > 0) { "no track" }
            ex.selectTrack(0)
            muxer = MediaMuxer(out, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
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
}
