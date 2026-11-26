package io.github.lokimdj.seogi.ui

import java.time.Instant
import java.time.ZoneId

/** Times and lengths the way the screens say them. */
object Format {
    /** `0:42:18`. */
    fun clock(ms: Long): String {
        val s = (ms / 1000).coerceAtLeast(0)
        return "%d:%02d:%02d".format(s / 3600, s / 60 % 60, s % 60)
    }

    /** `0:12:40` for a mark, the same as [clock]. */
    fun mark(ms: Long) = clock(ms)

    /** `45초`, `22분`, `1시간 48분`. */
    fun length(ms: Long): String {
        val s = (ms / 1000).coerceAtLeast(0)
        return when {
            s < 60 -> "${s}초"
            s < 3600 -> "${s / 60}분"
            s % 3600 / 60 == 0L -> "${s / 3600}시간"
            else -> "${s / 3600}시간 ${s % 3600 / 60}분"
        }
    }

    /** `14:30`. */
    fun hm(epochMs: Long, zone: ZoneId): String {
        val t = Instant.ofEpochMilli(epochMs).atZone(zone)
        return "%d:%02d".format(t.hour, t.minute)
    }

    /** `오늘 10:30`, `어제 16:00`, or `11/19 11:00`. */
    fun day(epochMs: Long, now: Long, zone: ZoneId, today: String, yesterday: String): String {
        val d = Instant.ofEpochMilli(epochMs).atZone(zone).toLocalDate()
        val n = Instant.ofEpochMilli(now).atZone(zone).toLocalDate()
        val prefix = when (d) {
            n -> today
            n.minusDays(1) -> yesterday
            else -> "${d.monthValue}/${d.dayOfMonth}"
        }
        return "$prefix ${hm(epochMs, zone)}"
    }
}
