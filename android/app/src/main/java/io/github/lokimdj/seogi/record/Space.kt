package io.github.lokimdj.seogi.record

/**
 * How much room a recording needs. At 64 kbps an hour is about 29 MB; the
 * stream and the saved .m4a exist side by side for a moment, so saving takes
 * twice the recording's size.
 */
object Space {
    const val MB = 1024L * 1024L
    /** Enough for a three-hour meeting and its save: 3 h × 29 MB × 2, rounded up. */
    const val TO_START = 200 * MB
    /** Below this while recording, stop and save what there is. */
    const val TO_KEEP_GOING = 40 * MB

    fun canStart(freeBytes: Long) = freeBytes >= TO_START

    /** Room left to save [streamBytes] (one more copy) and a little to spare. */
    fun mustStop(freeBytes: Long, streamBytes: Long) = freeBytes < maxOf(TO_KEEP_GOING, streamBytes + 10 * MB)
}
