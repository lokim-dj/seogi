package io.github.lokimdj.seogi.data

import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter

/** How a recording is named on the phone, in Drive and on screen. */
object FileNames {
    private val stamp = DateTimeFormatter.ofPattern("yyyy-MM-dd HHmm")
    private val forbidden = Regex("""[\\/:*?"<>|\p{Cntrl}]""")
    private const val MAX_TITLE = 80

    /** `YYYY-MM-DD HHMM 일정제목` without extension; the time alone when there is no event. */
    fun baseName(start: ZonedDateTime, eventTitle: String?): String {
        val title = eventTitle?.let(::safeTitle).orEmpty()
        val time = start.format(stamp)
        return if (title.isEmpty()) time else "$time $title"
    }

    /** What the lists show: the event's own title, or "14:05 녹음". */
    fun displayTitle(start: ZonedDateTime, eventTitle: String?): String =
        eventTitle?.trim()?.takeIf { it.isNotEmpty() }
            ?: "%d:%02d 녹음".format(start.hour, start.minute)

    internal fun safeTitle(raw: String): String =
        raw.replace(forbidden, " ")
            .replace(Regex("""\s+"""), " ")
            .trim()
            .take(MAX_TITLE)
            .trim()
            .trimEnd('.')
}
