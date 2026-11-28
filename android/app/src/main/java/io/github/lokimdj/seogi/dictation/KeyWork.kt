package io.github.lokimdj.seogi.dictation

/**
 * How long the floating key may listen or make text before it gives up
 * waiting, and what becomes of the text when it comes. Pure, so it is tested
 * on the computer.
 *
 * The key must always come back to rest: a speech engine that never answers
 * (stuck, or a phone so short of memory that it crawls) once left it turning
 * for good, on every screen (#96).
 */
object KeyWork {
    /** Room for opening the model (a second or two, longer when the phone is short of memory) on top of the run itself. */
    const val WORK_BASE_MS = 20_000L

    /**
     * Longest listening: the microphone stops on its own at
     * [Speech.MAX_SAMPLES]; this catches a microphone that stopped sending
     * sound before getting there.
     */
    const val LISTEN_CAP_MS = Speech.MAX_SAMPLES * 1000L / SpeechEngine.RATE + 5_000L

    /**
     * How long making text of [samples] may take before the key stops
     * waiting: [WORK_BASE_MS] plus as long as the speech itself. The engine
     * runs several times faster than speech, so this is reached only when
     * something is stuck.
     */
    fun workCapMs(samples: Int): Long = WORK_BASE_MS + samples.coerceAtLeast(0) * 1000L / SpeechEngine.RATE

    /**
     * How long a listening key waits, once no field is in sight, before it
     * goes (and listening stops and writes): longer than at rest, so the
     * keyboard folding away for a moment while the user speaks does not cut
     * the sentence.
     */
    const val LISTEN_HIDE_MS = 1_500L

    /**
     * Whether the key turns while new speech is made into text: not while
     * [inFlight] earlier runs are still under way. One the key stopped
     * waiting for still holds the engine (stuck, or crawling), and the new
     * one can only start after it, so the key rests at once and the words
     * are copied when their turn comes, instead of turning out its time cap
     * again on every try.
     */
    fun turns(inFlight: Int): Boolean = inFlight <= 0

    /** What is done with a run's result. */
    enum class End {
        /** Into the field with the cursor (or the clipboard, when none takes it). */
        TYPE,

        /** Onto the clipboard only. */
        COPY,

        /** Say it failed. */
        FAILED,

        /** Say nothing was heard. */
        HEARD_NOTHING,

        /** Nothing to do or say. */
        QUIET,
    }

    /**
     * What to do with the [text] of a run (null when it failed). [waited]: the
     * key was still waiting for it; false once it stopped waiting (the time
     * cap or a tap).
     * - Waited: type it, or say what went wrong.
     * - Not waited: the user was told the words would be copied, and may be
     *   in another app by now, so they go on the clipboard, never into a
     *   field. A failure is still said; nothing heard needs no word.
     */
    fun end(text: String?, waited: Boolean): End = when {
        text == null -> End.FAILED
        text.isEmpty() -> if (waited) End.HEARD_NOTHING else End.QUIET
        waited -> End.TYPE
        else -> End.COPY
    }
}
