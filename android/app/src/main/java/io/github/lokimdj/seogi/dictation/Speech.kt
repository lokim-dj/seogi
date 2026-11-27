package io.github.lokimdj.seogi.dictation

/** What happens to the sound before and the text after the engine. Pure, so it is tested on the computer. */
object Speech {
    /** Longest piece handed to the engine in one run (the model's comfortable window). */
    const val MAX_PIECE_SAMPLES = 25 * SpeechEngine.RATE

    /** Shorter than this is a slip of the finger, not speech. */
    const val MIN_SAMPLES = SpeechEngine.RATE * 3 / 10

    /** Listening stops on its own after this long. */
    const val MAX_SAMPLES = 120 * SpeechEngine.RATE

    private const val WINDOW = SpeechEngine.RATE / 10

    /**
     * Cuts long speech into pieces of at most [max] samples, each cut at the
     * quietest tenth of a second in the last 40% of the piece, so a word is
     * rarely split. Short speech comes back whole.
     */
    fun pieces(pcm: FloatArray, max: Int = MAX_PIECE_SAMPLES): List<FloatArray> {
        val out = mutableListOf<FloatArray>()
        var start = 0
        while (pcm.size - start > max) {
            val from = start + max * 6 / 10
            val to = start + max - WINDOW
            var best = to
            var bestEnergy = Double.MAX_VALUE
            var i = from
            while (i <= to) {
                var e = 0.0
                for (j in i until i + WINDOW) e += pcm[j] * pcm[j]
                if (e < bestEnergy) {
                    bestEnergy = e
                    best = i
                }
                i += WINDOW / 2
            }
            val cut = best + WINDOW / 2
            out += pcm.copyOfRange(start, cut)
            start = cut
        }
        if (start < pcm.size) out += pcm.copyOfRange(start, pcm.size)
        return out
    }

    /** The pieces' texts as one: trimmed, empty ones dropped, joined by a space. Punctuation is kept. */
    fun join(texts: List<String>): String = texts.map { it.trim() }.filter { it.isNotEmpty() }.joinToString(" ")
}

/** Text put into a field: where it goes and where the cursor ends up. */
data class Merged(val text: String, val cursor: Int)

object TextInsert {
    /**
     * [insert] put into [current] at the selection [selStart]..[selEnd]
     * (replacing what is selected). An unknown selection (-1) means the end.
     * A space goes between the text before and the new words, unless the text
     * before already ends in a space or a line break, or the new words begin
     * with punctuation.
     */
    fun merge(current: String, selStart: Int, selEnd: Int, insert: String): Merged {
        val n = current.length
        var a = if (selStart in 0..n) selStart else n
        var b = if (selEnd in 0..n) selEnd else a
        if (a > b) a = b.also { b = a }
        val before = current.substring(0, a)
        val after = current.substring(b)
        val gap = if (before.isNotEmpty() && !before.last().isWhitespace() && insert.firstOrNull()?.let { isOpening(it) } == true) " " else ""
        val put = gap + insert
        return Merged(before + put + after, before.length + put.length)
    }

    private fun isOpening(c: Char) = c.isLetterOrDigit() || c in "\"'(「『[“‘"
}

/** A text field in another app, as much of it as putting text in needs. */
interface TextField {
    val editable: Boolean
    /** A password field shows dots to accessibility, not its text, so it is never rewritten whole. */
    val password: Boolean
    /** The text, or empty when the field only shows its hint. */
    val text: String
    val selectionStart: Int
    val selectionEnd: Int
    fun setText(text: String): Boolean
    fun setCursor(at: Int)
    /** Puts [text] on the clipboard and asks the field to paste it. */
    fun paste(text: String): Boolean
}

object Inserter {
    /**
     * Puts [text] at the cursor of [field]. The whole new text first (most
     * fields); a paste when the field refuses that (some web pages and rich
     * editors, and password fields, whose text accessibility sees only as
     * dots). False when the field takes neither; the caller copies.
     */
    fun insert(field: TextField, text: String): Boolean {
        if (!field.editable) return false
        if (field.password) return field.paste(text)
        val merged = TextInsert.merge(field.text, field.selectionStart, field.selectionEnd, text)
        if (field.setText(merged.text)) {
            field.setCursor(merged.cursor)
            return true
        }
        return field.paste(text)
    }
}
