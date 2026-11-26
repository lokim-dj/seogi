package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.record.Adts
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.ByteArrayOutputStream
import java.io.File

/** Fix 1: the saved file must hold every frame of the stream before the stream goes. */
class AdtsTest {
    @get:Rule val tmp = TemporaryFolder()

    /** One ADTS frame of [length] bytes (7-byte header, no CRC). */
    private fun frame(length: Int): ByteArray {
        val b = ByteArray(length)
        b[0] = 0xFF.toByte()
        b[1] = 0xF1.toByte()
        b[2] = 0x50
        b[3] = (0x80 or ((length shr 11) and 0x03)).toByte()
        b[4] = ((length shr 3) and 0xFF).toByte()
        b[5] = (((length and 0x07) shl 5) or 0x1F).toByte()
        b[6] = 0xFC.toByte()
        return b
    }

    private fun stream(frames: Int, cutLast: Int = 0): File {
        val out = ByteArrayOutputStream()
        repeat(frames) { out.write(frame(180 + it % 7)) }
        val bytes = out.toByteArray()
        return File(tmp.root, "s.aac").apply { writeBytes(bytes.copyOf(bytes.size - cutLast)) }
    }

    @Test fun countsWholeFrames() = assertEquals(500, Adts.countFrames(stream(500)))

    @Test fun aFrameCutOffByACrashDoesNotCount() = assertEquals(499, Adts.countFrames(stream(500, cutLast = 40)))

    @Test fun aSaveMissingFramesIsNotComplete() {
        assertEquals(true, Adts.complete(savedFrames = 500, streamFrames = 500))
        assertEquals(true, Adts.complete(savedFrames = 498, streamFrames = 500))
        assertEquals(false, Adts.complete(savedFrames = 400, streamFrames = 500))
        assertEquals(false, Adts.complete(savedFrames = 0, streamFrames = 0))
    }
}
