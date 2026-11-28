package io.github.lokimdj.seogi.dictation

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder

/**
 * The microphone while the floating key listens: 16 kHz mono, kept in memory
 * only. Nothing is written to a file and nothing leaves the phone; the sound
 * is dropped as soon as it has become text.
 *
 * [onEnd] is called (on the microphone's thread) when listening stops on its
 * own: the longest speech was reached, or Android stopped sending sound (the
 * microphone was taken away or failed). Without it the key would listen on
 * for nothing.
 */
class Mic(private val onEnd: () -> Unit) {
    private var record: AudioRecord? = null
    private var reader: Thread? = null
    @Volatile private var running = false
    private var buf = ShortArray(SpeechEngine.RATE * 10)
    private var size = 0

    /** False when the microphone could not be opened (another app holds it, or Android refused). */
    @SuppressLint("MissingPermission") // checked by the caller
    fun start(): Boolean {
        val rate = SpeechEngine.RATE
        val min = AudioRecord.getMinBufferSize(rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        if (min <= 0) return false
        val rec = try {
            AudioRecord(MediaRecorder.AudioSource.VOICE_RECOGNITION, rate, AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT, maxOf(min, rate))
        } catch (e: Exception) {
            return false
        }
        if (rec.state != AudioRecord.STATE_INITIALIZED) {
            rec.release()
            return false
        }
        try {
            rec.startRecording()
        } catch (e: Exception) {
            rec.release()
            return false
        }
        if (rec.recordingState != AudioRecord.RECORDSTATE_RECORDING) {
            rec.release()
            return false
        }
        record = rec
        size = 0
        running = true
        reader = Thread({
            val chunk = ShortArray(rate / 10)
            while (running) {
                val n = rec.read(chunk, 0, chunk.size)
                if (n < 0) {
                    // An error, not our stop (that clears running first): what was heard so far is kept.
                    if (running) {
                        running = false
                        onEnd()
                    }
                    break
                }
                if (n == 0) continue
                synchronized(this) {
                    if (size + n > buf.size) buf = buf.copyOf(maxOf(buf.size * 2, size + n))
                    System.arraycopy(chunk, 0, buf, size, n)
                    size += n
                }
                if (size >= Speech.MAX_SAMPLES) {
                    running = false
                    onEnd()
                }
            }
        }, "seogi-mic").apply { start() }
        return true
    }

    /** Stops and hands over what was heard, as floats. */
    fun stop(): FloatArray {
        finish()
        return synchronized(this) {
            val out = FloatArray(size) { buf[it] / 32768f }
            size = 0
            buf = ShortArray(SpeechEngine.RATE * 10)
            out
        }
    }

    /** Stops and forgets what was heard. */
    fun cancel() {
        finish()
        synchronized(this) {
            size = 0
            buf = ShortArray(SpeechEngine.RATE * 10)
        }
    }

    private fun finish() {
        running = false
        val rec = record ?: return
        record = null
        try { rec.stop() } catch (_: Exception) {}
        try { reader?.join(1000) } catch (_: InterruptedException) {}
        reader = null
        rec.release()
    }
}
