package io.github.lokimdj.seogi.record

/** One event from the phone's calendar. */
data class CalendarEvent(val title: String, val begin: Long, val end: Long)

/** Which event a recording or the home line belongs to. */
object CalendarPick {
    /** An event may start this much after the recording and still name it. */
    const val SOON_MS = 10 * 60 * 1000L

    /**
     * The event a recording started at [now] belongs to: one under way or
     * starting within ten minutes, the one whose start is closest to [now].
     */
    fun current(events: List<CalendarEvent>, now: Long): CalendarEvent? =
        events
            .filter { it.title.isNotBlank() && it.begin <= now + SOON_MS && it.end > now }
            .minByOrNull { kotlin.math.abs(it.begin - now) }

    /** The home's line: the current event, or else the next one still to come. */
    fun next(events: List<CalendarEvent>, now: Long): CalendarEvent? =
        current(events, now) ?: events.filter { it.title.isNotBlank() && it.begin > now }.minByOrNull { it.begin }
}
