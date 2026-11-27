package io.github.lokimdj.seogi.dictation

/**
 * Spoken numbers written as digits: "이십 초" → "20초", "삼천오백 원" →
 * "3,500원", "오후 세 시 반" → "오후 3시 반".
 *
 * A line-for-line port of the desktop's `src-tauri/src/seogi/numbers.rs`, so
 * the phone and the computer write the same words the same way. Both are
 * tested against one file of sentences (`src-tauri/src/seogi/numbers.cases.json`):
 * a rule changed on one side breaks the other side's test. Change both
 * together, and the rules only there; the reasons for each are written in the
 * Rust file.
 *
 * The text is read as Unicode code points, as Rust reads `char`s, and
 * whitespace and "letter or digit" follow Rust's definitions, so punctuation
 * and odd characters split words in exactly the same places. Punctuation is
 * kept as it was.
 */
object Numbers {
    private enum class Kind { SINO, NATIVE }

    private class Spoken(
        val value: Long,
        val fraction: String?,
        val kind: Kind,
        /** The number's only syllable, when it is said in one. */
        val lone: Int?,
        val first: String,
    )

    private class Measure(
        val word: String,
        val native: Boolean,
        val amount: Boolean,
        val loneSino: String,
        val loneNative: String,
        val max: Long,
        val measure: Boolean,
    ) {
        val cps: IntArray = word.codePoints().toArray()
    }

    private const val ANY = Long.MAX_VALUE
    private const val ALL_SINO = "일삼사오육칠팔구십백천"

    private fun u(word: String, native: Boolean, amount: Boolean, loneSino: String, loneNative: String, max: Long, measure: Boolean) =
        Measure(word, native, amount, loneSino, loneNative, max, measure)

    private val UNITS = listOf(
        u("퍼센트", false, false, "일이삼사오육칠팔구십백", "", ANY, true),
        u("프로", false, false, "일삼사오육칠팔구십백", "", ANY, true),
        u("개월", true, false, "일이삼사오육칠팔구십", "두세", 1200, false),
        u("인분", false, true, "일이삼사오육칠팔구십", "", ANY, false),
        u("주년", false, false, "일삼사오육칠팔구십백", "", ANY, false),
        u("학년", false, false, "일삼사오육", "", 6, false),
        u("센티미터", false, true, ALL_SINO, "", ANY, true),
        u("밀리미터", false, true, ALL_SINO, "", ANY, true),
        u("킬로미터", false, true, ALL_SINO, "", ANY, true),
        u("킬로그램", false, true, ALL_SINO, "", ANY, true),
        u("미터", false, true, ALL_SINO, "", ANY, true),
        u("센티", false, true, ALL_SINO, "", ANY, true),
        u("센치", false, true, ALL_SINO, "", ANY, true),
        u("밀리", false, true, ALL_SINO, "", ANY, true),
        u("킬로", false, true, ALL_SINO, "", ANY, true),
        u("키로", false, true, ALL_SINO, "", ANY, true),
        u("그램", false, true, ALL_SINO, "", ANY, true),
        u("리터", false, true, ALL_SINO, "", ANY, true),
        u("페이지", false, false, "", "", ANY, false),
        u("주일", false, false, "", "", ANY, false),
        u("호선", false, false, "일삼사오육칠팔구", "", 20, false),
        u("번지", false, false, "", "", ANY, false),
        u("번째", false, false, "", "", ANY, false),
        u("달러", false, true, "십백천", "", ANY, true),
        u("세기", false, false, "", "", 30, false),
        u("시간", true, true, "", "두세", ANY, true),
        u("초", false, false, "", "", ANY, true),
        u("분", false, false, "", "", ANY, false),
        u("시", true, false, "", "두세", 24, false),
        u("년", false, false, ALL_SINO, "", ANY, false),
        u("월", false, false, "", "", 12, false),
        u("일", false, false, "", "", 366, false),
        u("주", true, false, "", "", ANY, false),
        u("원", false, true, "십백천", "", ANY, false),
        u("층", false, false, "일삼사오육구십", "", 300, false),
        u("평", false, true, "", "", ANY, true),
        u("동", false, false, "", "", 9999, false),
        u("호", false, false, "", "", 9999, false),
        u("차", false, false, "", "", ANY, false),
        u("회", false, false, "", "", ANY, false),
        u("번", true, false, "", "세", ANY, false),
        u("위", false, false, "", "", ANY, false),
        u("세", false, false, "", "", 150, false),
        u("점", false, false, "", "", ANY, false),
        u("도", false, false, "", "", ANY, true),
        u("쪽", false, false, "", "", ANY, false),
        u("배", true, false, "", "두세", ANY, true),
        u("개", true, true, "백천", "두세열", ANY, false),
        u("명", true, true, "백천", "두세열", ANY, false),
        u("살", true, false, "", "두세열쉰", 150, false),
        u("달", true, false, "", "", ANY, false),
        u("대", true, true, "", "두세", ANY, false),
        u("장", true, true, "", "두세열", ANY, false),
        u("권", true, true, "", "두세열", ANY, false),
        u("잔", true, true, "", "두세열", ANY, false),
        u("마리", true, true, "", "두세열", ANY, false),
        u("군데", true, true, "", "두세열", ANY, false),
        u("곳", true, true, "", "두세열", ANY, false),
        u("가지", true, true, "", "두세열", ANY, false),
    )

    private val PARTICLES = setOf(
        "이", "가", "은", "는", "을", "를", "의", "에", "에서", "에는", "에도", "에서는", "에게", "도", "만",
        "만에", "까지", "까지는", "부터", "부터는", "씩", "쯤", "째", "짜리", "와", "과", "로", "으로", "으로는",
        "이나", "이면", "이고", "이라", "이라서", "이라고", "이요", "요", "입니다", "이에요", "예요", "인데",
        "이죠", "죠", "이야", "이랑", "랑", "하고", "정도", "간", "경", "께", "마다", "밖에", "뿐", "어치",
        "라서", "이니까", "니까", "이었어요", "였어요", "이었는데", "였는데", "까지요", "부터요", "이서", "에요",
    )

    private val UNIT_ENDINGS = mapOf(
        "년" to listOf("대", "생", "차", "말", "초", "반", "도"),
        "월" to listOf("말", "초", "중순"),
        "주" to listOf("차"),
        "일" to listOf("차"),
        "회" to listOf("차"),
        "평" to listOf("형"),
        "시" to listOf("반"),
        "시간" to listOf("반"),
        "개월" to listOf("반"),
    )

    private val UNIT_WORDS = setOf(
        "시도", "시가", "세로", "세가", "세대", "도로", "분과", "분만", "차이", "차도", "차로", "회의",
        "회가", "평가", "평이", "주가", "주요", "주의", "주도", "호가", "위도", "점도", "명도", "대가",
        "대로", "장도", "층간",
    )

    private val NUMBER_WORDS = mapOf(
        "천사" to listOf(),
        "백사" to listOf(),
        "백일" to listOf(),
        "일조" to listOf(),
        "이조" to listOf(),
        "구조" to listOf(),
        "일천" to listOf(),
        "오만" to listOf("원"),
        "이만" to listOf("원"),
        "만" to listOf("원", "명", "개"),
        "천만" to listOf("원", "명"),
        "백만" to listOf("원", "명"),
        "이천" to listOf("원", "년"),
    )

    private val RANGE_BEFORE = setOf("몇", "수", "한", "두", "세", "네", "서너", "너덧", "너댓", "여러", "대여섯", "예닐곱")

    private val PERCENT_BEFORE = setOf(
        "약", "대략", "거의", "무려", "최대", "최소", "총", "겨우", "고작", "딱", "연", "월", "평균", "대충", "얼추",
    )

    private val PERCENT_AFTER = listOf(
        "정도", "쯤", "가량", "이상", "이하", "미만", "초과", "넘게", "넘어", "넘는", "넘었", "할인", "인상", "인하",
        "올라", "올랐", "올려", "올린", "내렸", "내려", "내린", "떨어", "늘었", "늘어", "늘려", "줄었", "줄어", "줄여",
        "남았", "남아", "남은", "빠졌", "빠져", "확실", "증가", "감소", "차이", "수수료", "이자", "금리",
    )

    private val TIMES_OF_DAY = setOf("오전", "오후", "새벽", "아침", "저녁", "밤", "낮")

    private val DURATION_AFTER = setOf(
        "동안", "동안에", "동안은", "뒤", "뒤에", "뒤에는", "후", "후에", "후에는", "전", "전에", "전에는", "만에",
        "이내", "이내에", "째",
    )

    private val DURATION_UNITS = setOf("시간", "분", "초", "일", "주", "개월", "년", "달")

    private val HALF_AFTER = setOf("반", "반에", "반까지", "반부터", "반쯤", "반이에요", "반입니다")

    private val CHAINS = mapOf(
        "년" to listOf("월", "개월"),
        "월" to listOf("일"),
        "시" to listOf("분"),
        "시간" to listOf("분"),
        "분" to listOf("초"),
        "동" to listOf("호"),
        "층" to listOf("호"),
    )

    /** In this order: the longer words first where one starts another. */
    private val TENS = listOf(
        "스물" to 20L, "스무" to 20L, "서른" to 30L, "마흔" to 40L, "예순" to 60L, "일흔" to 70L,
        "여든" to 80L, "아흔" to 90L, "열" to 10L, "쉰" to 50L,
    )

    private val ONES = listOf(
        "다섯" to 5L, "여섯" to 6L, "일곱" to 7L, "여덟" to 8L, "아홉" to 9L, "한" to 1L, "두" to 2L, "세" to 3L, "네" to 4L,
    )

    private const val MAX_NUMBER_WORDS = 4

    // --- characters, as Rust sees them ---

    private fun str(cps: IntArray): String = String(cps, 0, cps.size)

    private fun cp(c: Char): Int = c.code

    private fun isHangul(c: Int) = c in 0xAC00..0xD7A3

    /** Rust's `char::is_whitespace`: the Unicode White_Space property. */
    private fun isWhitespace(c: Int) = c in 0x09..0x0D || c == 0x20 || c == 0x85 || c == 0xA0 || c == 0x1680 ||
        c in 0x2000..0x200A || c == 0x2028 || c == 0x2029 || c == 0x202F || c == 0x205F || c == 0x3000

    /** Rust's `char::is_alphanumeric`: Alphabetic, or a number (Nd, Nl, No). */
    private fun isAlphanumeric(c: Int): Boolean {
        if (Character.isAlphabetic(c)) return true
        val t = Character.getType(c)
        return t == Character.DECIMAL_DIGIT_NUMBER.toInt() || t == Character.LETTER_NUMBER.toInt() ||
            t == Character.OTHER_NUMBER.toInt()
    }

    private fun isAsciiDigit(c: Int) = c in '0'.code..'9'.code

    private fun sinoDigit(c: Int): Long? = when (c) {
        cp('일') -> 1
        cp('이') -> 2
        cp('삼') -> 3
        cp('사') -> 4
        cp('오') -> 5
        cp('육') -> 6
        cp('칠') -> 7
        cp('팔') -> 8
        cp('구') -> 9
        else -> null
    }

    private fun sinoPlace(c: Int): Long? = when (c) {
        cp('십') -> 10
        cp('백') -> 100
        cp('천') -> 1_000
        else -> null
    }

    private fun sinoBig(c: Int): Long? = when (c) {
        cp('만') -> 10_000
        cp('억') -> 100_000_000
        cp('조') -> 1_000_000_000_000
        else -> null
    }

    private fun isSino(c: Int) = sinoDigit(c) != null || sinoPlace(c) != null || sinoBig(c) != null

    private fun isPlaceOrBig(c: Int) = sinoPlace(c) != null || sinoBig(c) != null

    private fun isJust(s: IntArray, c: Char) = s.size == 1 && s[0] == c.code

    private fun allSino(s: IntArray) = s.all { isSino(it) }

    /** Rust's checked u64 arithmetic: null where it would not fit. */
    private fun checkedAdd(a: Long, b: Long): Long? = if (a < 0 || b < 0 || a > Long.MAX_VALUE - b) null else a + b

    private fun checkedMul(a: Long, b: Long): Long? = try { Math.multiplyExact(a, b) } catch (e: ArithmeticException) { null }

    private fun sinoValue(words: IntArray): Long? {
        if (words.isEmpty()) return null
        var value = 0L
        var group = 0L
        var pending: Long? = null
        var lastPlace = Long.MAX_VALUE
        var lastBig = Long.MAX_VALUE
        for ((i, c) in words.withIndex()) {
            val d = sinoDigit(c)
            val p = sinoPlace(c)
            val b = sinoBig(c)
            if (d != null) {
                if (pending != null) return null
                pending = d
            } else if (p != null) {
                if (p >= lastPlace) return null
                group += (pending ?: 1L) * p
                pending = null
                lastPlace = p
            } else if (b != null) {
                if (b >= lastBig) return null
                var g = group + (pending ?: 0L)
                pending = null
                if (g == 0L) {
                    val moreHasPlace = (i + 1 until words.size).any { sinoPlace(words[it]) != null }
                    if (i != 0 || b != 10_000L || !moreHasPlace) return null
                    g = 1
                }
                value = checkedAdd(value, checkedMul(g, b) ?: return null) ?: return null
                group = 0
                lastPlace = Long.MAX_VALUE
                lastBig = b
            }
        }
        return checkedAdd(value, group + (pending ?: 0L))
    }

    private fun startsWithAt(s: IntArray, at: Int, w: String): Boolean {
        val wc = w.codePoints().toArray()
        if (s.size < at + wc.size) return false
        for (k in wc.indices) if (s[at + k] != wc[k]) return false
        return true
    }

    /** A native number at the start of [s]: its value and how many letters it takes. */
    private fun nativePrefix(s: IntArray): Pair<Long, Int>? {
        var value = 0L
        var at = 0
        val tens = TENS.firstOrNull { (w, _) -> startsWithAt(s, 0, w) }
        if (tens != null) {
            value = tens.second
            at = tens.first.length
            if (tens.first == "스무") return value to at
        }
        val ones = ONES.firstOrNull { (w, _) -> startsWithAt(s, at, w) }
        if (ones != null) {
            value += ones.second
            at += ones.first.length
        }
        return if (at > 0) value to at else null
    }

    private fun nativeWhole(s: IntArray): Long? = nativePrefix(s)?.let { (v, n) -> if (n == s.size) v else null }

    private fun isNumberWord(s: IntArray): Boolean {
        if (s.isEmpty()) return false
        val digits = s.all { isAsciiDigit(it) || it == ','.code || it == '.'.code }
        return digits || nativeWhole(s) != null || s.all { isSino(it) || it == '영'.code || it == '공'.code }
    }

    private class Word(val core: Int, val end: Int, val closed: Boolean)

    private fun splitWords(chars: IntArray): List<Word> {
        val words = ArrayList<Word>()
        var i = 0
        while (i < chars.size) {
            if (isWhitespace(chars[i])) {
                i++
                continue
            }
            val start = i
            while (i < chars.size && !isWhitespace(chars[i])) i++
            var core = start
            while (core < i && !isAlphanumeric(chars[core])) core++
            val closed = !isAlphanumeric(chars[i - 1])
            words += Word(core, i, closed)
        }
        return words
    }

    private fun letters(chars: IntArray, word: Word): IntArray {
        var end = word.end
        while (end > word.core && !isAlphanumeric(chars[end - 1])) end--
        return chars.copyOfRange(word.core, end)
    }

    private fun unitThenEnd(s: IntArray): Pair<Measure, Int>? {
        var best: Measure? = null
        for (unit in UNITS) {
            val w = unit.cps
            if (s.size >= w.size && startsWithAt(s, 0, unit.word) && (best == null || best.cps.size < w.size)) best = unit
        }
        val unit = best ?: return null
        val n = unit.cps.size
        val rest = String(s, n, s.size - n)
        val whole = str(s)
        if (whole in UNIT_WORDS) return null
        if (rest.isEmpty() || rest in PARTICLES) return unit to n
        for (ending in UNIT_ENDINGS[unit.word].orEmpty()) {
            if (rest.startsWith(ending)) {
                val after = rest.substring(ending.length)
                if (after.isEmpty() || after in PARTICLES) return unit to n
            }
        }
        return null
    }

    /** [text] with spoken numbers that come before a unit written as digits. */
    fun toDigits(text: String): String {
        val chars = text.codePoints().toArray()
        val words = splitWords(chars)
        val edits = ArrayList<Found>()
        var last: Pair<Int, String>? = null
        var i = 0
        while (i < words.size) {
            val found = convertAt(chars, words, i, last)
            if (found != null) {
                edits += found
                last = found.unitWord to found.unit
                i = found.unitWord + 1
            } else {
                i++
            }
        }
        if (edits.isEmpty()) return text
        val out = StringBuilder(text.length)
        var at = 0
        for (e in edits) {
            for (k in at until e.start) out.appendCodePoint(chars[k])
            out.append(e.text)
            at = e.end
        }
        for (k in at until chars.size) out.appendCodePoint(chars[k])
        return out.toString()
    }

    private class Found(val start: Int, val end: Int, val text: String, val unitWord: Int, val unit: String)

    private fun convertAt(chars: IntArray, words: List<Word>, i: Int, last: Pair<Int, String>?): Found? {
        val word = letters(chars, words[i])
        if (word.isEmpty() || !isHangul(word[0])) return null
        if (i > 0 && !words[i - 1].closed) {
            val prev = letters(chars, words[i - 1])
            if (str(prev) in RANGE_BEFORE || isNumberWord(prev)) return null
        }
        return spaced(chars, words, i, last) ?: attached(chars, words, i)
    }

    private fun spaced(chars: IntArray, words: List<Word>, i: Int, last: Pair<Int, String>?): Found? {
        val first = letters(chars, words[i])
        val firstText = str(first)
        fun open(k: Int) = k < words.size && !words[k - 1].closed

        val candidates = ArrayList<Pair<Spoken, Int>>()
        val native = nativeWhole(first)
        if (native != null) {
            var value = native
            var next = i + 1
            val isTens = TENS.any { (w, _) -> w != "스무" && w == firstText }
            if (isTens && open(next)) {
                val following = str(letters(chars, words[next]))
                val ones = ONES.firstOrNull { (w, _) -> w == following }
                if (ones != null) {
                    value += ones.second
                    next++
                }
            }
            val syllables = (i until next).sumOf { letters(chars, words[it]).size }
            candidates += Spoken(value, null, Kind.NATIVE, if (syllables == 1) first[0] else null, firstText) to next
        }
        if (allSino(first) || isJust(first, '영')) {
            val zero = isJust(first, '영')
            var syllables = first
            var next = i + 1
            while (next - i < MAX_NUMBER_WORDS && open(next) && !zero) {
                val following = letters(chars, words[next])
                val joins = syllables.isNotEmpty() && isPlaceOrBig(syllables.last()) &&
                    following.isNotEmpty() && allSino(following) && following.any { isPlaceOrBig(it) }
                if (!joins) break
                syllables += following
                next++
            }
            val value = if (zero) 0L else sinoValue(syllables)
            if (value != null) {
                var fraction: String? = null
                var after = next
                if (open(after + 1) && isJust(letters(chars, words[after]), '점')) {
                    val digits = letters(chars, words[after + 1])
                    val read = StringBuilder()
                    var ok = true
                    for (c in digits) {
                        val d: Long? = if (c == '영'.code || c == '공'.code) 0L else sinoDigit(c)
                        if (d == null) {
                            ok = false
                            break
                        }
                        read.append(d)
                    }
                    if (ok && read.isNotEmpty()) {
                        fraction = read.toString()
                        after += 2
                    }
                }
                if (!zero || fraction != null) {
                    val lone = if (syllables.size == 1 && fraction == null) syllables[0] else null
                    candidates += Spoken(value, fraction, Kind.SINO, lone, firstText) to after
                }
            }
        }

        for ((number, unitWord) in candidates) {
            if (!open(unitWord)) continue
            val unitLetters = letters(chars, words[unitWord])
            val (unit, unitLen) = unitThenEnd(unitLetters) ?: continue
            if (!fits(number, unit) || !unsureUnitsClear(chars, words, i, unitWord, unit, number)) continue
            val unitAlone = unitLen == unitLetters.size
            if (unitAlone && open(unitWord + 1)) {
                val next = letters(chars, words[unitWord + 1])
                val nextText = str(next)
                val bareUnit = UNITS.any { it.word == nextText }
                if (bareUnit && !continues(chars, words, unitWord, unit)) continue
                if ((unit.word == "대" || unit.word == "점") && isNumberWord(withoutParticle(next))) continue
            }
            val around = Around.read(chars, words, i, unitWord, unit, last, unitAlone)
            if (!allowed(number, unit, around)) continue
            val start = words[i].core
            val end = words[unitWord].core + unitLen
            return Found(start, end, written(number, unit), unitWord, unit.word)
        }
        return null
    }

    private fun unsureUnitsClear(chars: IntArray, words: List<Word>, i: Int, unitWord: Int, unit: Measure, number: Spoken): Boolean =
        when {
            unit.word == "프로" -> plainBefore(chars, words, i) && amountAfter(chars, words, unitWord)
            unit.word == "층" && number.lone == '구'.code -> plainBefore(chars, words, i)
            unit.word == "번" && number.lone != null -> i == 0 || letters(chars, words[i - 1]).firstOrNull() != '번'.code
            else -> true
        }

    private fun amountAfter(chars: IntArray, words: List<Word>, unitWord: Int): Boolean {
        if (words[unitWord].closed || unitWord + 1 >= words.size) return false
        val next = str(letters(chars, words[unitWord + 1]))
        return PERCENT_AFTER.any { next.startsWith(it) }
    }

    private fun plainBefore(chars: IntArray, words: List<Word>, i: Int): Boolean {
        if (i == 0 || words[i - 1].closed) return true
        val prev = letters(chars, words[i - 1])
        if (prev.isEmpty() || !prev.all { isHangul(it) }) return false
        return str(prev) in PERCENT_BEFORE || withoutParticle(prev).size < prev.size
    }

    private fun withoutParticle(word: IntArray): IntArray {
        val text = str(word)
        val longest = PARTICLES
            .filter { text.length > it.length && text.endsWith(it) }
            .maxOfOrNull { it.codePointCount(0, it.length) } ?: 0
        return word.copyOfRange(0, word.size - longest)
    }

    private fun continues(chars: IntArray, words: List<Word>, unitWord: Int, unit: Measure): Boolean {
        val nextUnits = CHAINS[unit.word] ?: return false
        if (unitWord + 2 >= words.size || words[unitWord].closed || words[unitWord + 1].closed) return false
        val number = letters(chars, words[unitWord + 1])
        return allSino(number) && sinoValue(number) != null &&
            unitThenEnd(letters(chars, words[unitWord + 2]))?.let { it.first.word in nextUnits } == true
    }

    private fun attached(chars: IntArray, words: List<Word>, i: Int): Found? {
        val word = letters(chars, words[i])
        val candidates = ArrayList<Pair<Spoken, Int>>()
        nativePrefix(word)?.let { (value, n) ->
            if (n >= 2 && n < word.size) {
                candidates += Spoken(value, null, Kind.NATIVE, null, String(word, 0, n)) to n
            }
        }
        val n = word.takeWhile { isSino(it) }.size
        if (n >= 2 && n < word.size) {
            sinoValue(word.copyOfRange(0, n))?.let { value ->
                candidates += Spoken(value, null, Kind.SINO, null, String(word, 0, n)) to n
            }
        }
        for ((number, len) in candidates) {
            val (unit, unitLen) = unitThenEnd(word.copyOfRange(len, word.size)) ?: continue
            if (unit.word == "도" || !fits(number, unit) || !unsureUnitsClear(chars, words, i, i, unit, number)) continue
            if (number.kind == Kind.NATIVE && !unit.native) continue
            val start = words[i].core
            return Found(start, start + len + unitLen, written(number, unit), i, unit.word)
        }
        return null
    }

    private fun fits(number: Spoken, unit: Measure): Boolean {
        val only = NUMBER_WORDS[number.first]
        if (only != null && unit.word !in only) return false
        if (number.fraction != null && !unit.measure) return false
        if (number.value > unit.max) return false
        if (number.value >= 10_000 && !unit.amount) return false
        if (number.value >= 100_000_000 && unit.word !in setOf("원", "달러", "명")) return false
        return true
    }

    private class Around(
        val chain: Boolean,
        val timeOfDay: Boolean,
        val half: Boolean,
        val minutes: Boolean,
        val duration: Boolean,
    ) {
        companion object {
            fun read(
                chars: IntArray,
                words: List<Word>,
                i: Int,
                unitWord: Int,
                unit: Measure,
                last: Pair<Int, String>?,
                unitAlone: Boolean,
            ): Around {
                val prev = if (i > 0 && !words[i - 1].closed) letters(chars, words[i - 1]) else null
                val prevUnit: String? = if (last != null && i > 0 && last.first == i - 1 && prev != null) {
                    last.second
                } else {
                    prev?.let { p ->
                        val digits = p.takeWhile { isAsciiDigit(it) }.size
                        if (digits > 0 && digits < p.size) String(p, digits, p.size - digits) else null
                    }
                }
                val chain = prevUnit != null && CHAINS[prevUnit]?.contains(unit.word) == true
                val prevWord = prev?.let { str(it) } ?: ""
                val next: String? = if (unitAlone && unitWord + 1 < words.size && !words[unitWord].closed) {
                    str(letters(chars, words[unitWord + 1]))
                } else {
                    null
                }
                fun nextIs(list: Set<String>) = next != null && next in list
                val pair = when (unit.word) {
                    "시" -> "분" to 59L
                    "월" -> "일" to 31L
                    else -> null
                }
                val minutes = pair != null && next != null && run {
                    val n = next.codePoints().toArray()
                    allSino(n) && (sinoValue(n)?.let { it <= pair.second } == true)
                } && unitWord + 2 < words.size && !words[unitWord + 1].closed &&
                    unitThenEnd(letters(chars, words[unitWord + 2]))?.let { it.first.word == pair.first } == true
                return Around(
                    chain = chain,
                    timeOfDay = unit.word == "시" && prevWord in TIMES_OF_DAY,
                    half = (unit.word == "시" || unit.word == "시간") && nextIs(HALF_AFTER),
                    minutes = minutes,
                    duration = unit.word in DURATION_UNITS && nextIs(DURATION_AFTER),
                )
            }
        }
    }

    private fun allowed(number: Spoken, unit: Measure, around: Around): Boolean {
        if (number.kind == Kind.NATIVE && !unit.native) return false
        val lone = number.lone ?: return true
        val hour = around.timeOfDay || around.half || around.minutes
        return when (lone) {
            '한'.code -> when (unit.word) {
                "시" -> hour
                "시간" -> around.half || around.duration
                else -> false
            }
            '네'.code -> unit.word == "시" && hour
            '이'.code -> around.chain || unit.word in setOf("퍼센트", "개월", "인분") || (unit.word == "월" && around.minutes)
            else -> {
                val list = if (number.kind == Kind.SINO) unit.loneSino else unit.loneNative
                val safe = list.codePoints().anyMatch { it == lone }
                safe || around.chain || (unit.word == "시" && hour) || (unit.word == "월" && around.minutes) || around.duration
            }
        }
    }

    private fun grouped(n: Long, comma: Boolean): String {
        val plain = n.toString()
        if (!comma || plain.length <= 3) return plain
        val out = StringBuilder()
        for ((i, c) in plain.withIndex()) {
            if (i > 0 && (plain.length - i) % 3 == 0) out.append(',')
            out.append(c)
        }
        return out.toString()
    }

    private fun written(number: Spoken, unit: Measure): String {
        val unitText = if (unit.word == "퍼센트" || unit.word == "프로") "%" else unit.word
        number.fraction?.let { return "${number.value}.$it$unitText" }
        val value = number.value
        if (value < 10_000) return grouped(value, unit.amount) + unitText
        val parts = ArrayList<String>()
        for ((size, word) in listOf(1_000_000_000_000L to "조", 100_000_000L to "억", 10_000L to "만")) {
            val part = if (word == "조") value / size else (value / size) % 10_000
            if (part > 0) parts += grouped(part, true) + word
        }
        val rest = value % 10_000
        return if (rest > 0) {
            parts += grouped(rest, true) + unitText
            parts.joinToString(" ")
        } else {
            "${parts.joinToString(" ")} $unitText"
        }
    }
}
