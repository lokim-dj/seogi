package io.github.lokimdj.seogi.record

import java.io.File

/**
 * Reads an ADTS AAC stream's frame headers, without decoding, to know how
 * many whole frames it holds. A frame cut off by a crash at the end does not
 * count.
 */
object Adts {
    fun countFrames(file: File): Int = file.inputStream().buffered(256 * 1024).use { input ->
        val header = ByteArray(7)
        val body = ByteArray(8192) // a frame is at most 8191 bytes
        var frames = 0
        while (true) {
            if (input.readNBytes(header, 0, 7) < 7) break
            val sync = (header[0].toInt() and 0xFF) == 0xFF && (header[1].toInt() and 0xF0) == 0xF0
            if (!sync) break
            val length = ((header[3].toInt() and 0x03) shl 11) or
                ((header[4].toInt() and 0xFF) shl 3) or
                ((header[5].toInt() and 0xE0) ushr 5)
            if (length < 7) break
            // Read, not skip: a file stream's skip may go past the end silently.
            if (input.readNBytes(body, 0, length - 7) < length - 7) break // the last frame was cut off
            frames++
        }
        frames
    }

    /**
     * The saved file holds the recording when it has every whole frame of the
     * stream (a frame or two of slack for the very end).
     */
    fun complete(savedFrames: Int, streamFrames: Int) = streamFrames > 0 && savedFrames >= streamFrames - 2
}
