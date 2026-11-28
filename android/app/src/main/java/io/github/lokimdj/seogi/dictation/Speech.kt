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

    /**
     * What is typed: the pieces joined, then spoken numbers before a unit
     * written as digits by the computer's rules ("세 시 반" → "3시 반", see
     * [Numbers]). Punctuation is kept.
     */
    fun written(texts: List<String>): String = Numbers.toDigits(join(texts))
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

/**
 * Which field the spoken text goes into, in this order:
 * 1. the field that has the cursor now, if it takes text;
 * 2. else the last field that took text while the user was typing, if it is
 *    still on the screen (touching the key or the keyboard closing can move
 *    the focus away); it gets the focus back first;
 * 3. else the field inside the focused part, when that part takes no text
 *    itself ([inside], see [InnerField]); asked only here, as finding it
 *    walks the part's children;
 * 4. else none: the text is copied, and [Choice.why] says why for the log.
 */
object InsertTarget {
    data class Choice<F>(val field: F?, val refocus: Boolean, val why: String)

    fun <F> pick(
        focused: F?,
        remembered: F?,
        editable: (F) -> Boolean,
        onScreen: (F) -> Boolean,
        inside: (F) -> F? = { null },
    ): Choice<F> {
        if (focused != null && editable(focused)) return Choice(focused, refocus = false, why = "focused field")
        val now = when {
            focused == null -> "no focused field"
            else -> "focused field takes no text"
        }
        val rememberedNot = when {
            remembered == null -> "none remembered"
            remembered == focused -> "the remembered one is it"
            !onScreen(remembered) -> "the remembered one is gone"
            !editable(remembered) -> "the remembered one takes no text"
            else -> return Choice(remembered, refocus = true, why = "$now, using the remembered field")
        }
        val inner = focused?.let(inside)
        if (inner != null && editable(inner)) {
            return Choice(inner, refocus = false, why = "$now, $rememberedNot, using the field inside it")
        }
        val innerNot = if (focused == null) "" else ", none inside it"
        return Choice(null, false, "$now, $rememberedNot$innerNot")
    }
}

/**
 * The text field inside a focused part that takes no text itself. Some apps
 * give the input focus to a scrolling frame around the field (Samsung Notes'
 * body: a ScrollView with the EditText inside, which never reports a cursor
 * change). Among the fields under [top], in this order:
 * 1. the one with the input focus, if it is the only one;
 * 2. else the one with a cursor position, if it is the only one;
 * 3. else the only field there is;
 * 4. else none (several fields and nothing tells which): the text is copied.
 *
 * At most [limit] parts are looked at, so a long list cannot stall the key;
 * when the walk stops early only rule 1 is used (a field could be unseen,
 * but the input focus is never on two).
 */
object InnerField {
    fun <F> pick(
        top: F,
        children: (F) -> List<F>,
        editable: (F) -> Boolean,
        focused: (F) -> Boolean,
        hasCursor: (F) -> Boolean,
        limit: Int = 300,
    ): F? {
        val fields = ArrayList<F>()
        val queue = ArrayDeque(children(top))
        var seen = 0
        while (queue.isNotEmpty()) {
            if (seen >= limit) break
            val n = queue.removeFirst()
            seen++
            // A field's own parts are not fields of their own.
            if (editable(n)) fields.add(n) else queue.addAll(children(n))
        }
        val complete = queue.isEmpty()
        fields.filter(focused).singleOrNull()?.let { return it }
        if (!complete) return null
        fields.filter(hasCursor).singleOrNull()?.let { return it }
        return fields.singleOrNull()
    }
}
