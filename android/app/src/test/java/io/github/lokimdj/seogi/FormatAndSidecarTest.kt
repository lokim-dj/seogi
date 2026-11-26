package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.data.Sidecar
import io.github.lokimdj.seogi.ui.Format
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Test
import java.time.ZoneId
import java.time.ZonedDateTime

class FormatAndSidecarTest {
    @Test fun clock() {
        assertEquals("0:42:18", Format.clock((42 * 60 + 18) * 1000L))
        assertEquals("2:03:04", Format.clock(((2 * 60 + 3) * 60 + 4) * 1000L))
    }

    @Test fun length() {
        assertEquals("45초", Format.length(45_000))
        assertEquals("22분", Format.length(22 * 60_000L))
        assertEquals("1시간 48분", Format.length(108 * 60_000L))
        assertEquals("2시간", Format.length(120 * 60_000L))
    }

    @Test fun day() {
        val z = ZoneId.of("Asia/Seoul")
        val now = ZonedDateTime.of(2026, 11, 26, 15, 0, 0, 0, z).toInstant().toEpochMilli()
        fun at(d: Int, h: Int, m: Int) = ZonedDateTime.of(2026, 11, d, h, m, 0, 0, z).toInstant().toEpochMilli()
        assertEquals("오늘 10:30", Format.day(at(26, 10, 30), now, z, "오늘", "어제"))
        assertEquals("어제 16:00", Format.day(at(25, 16, 0), now, z, "오늘", "어제"))
        assertEquals("11/19 9:05", Format.day(at(19, 9, 5), now, z, "오늘", "어제"))
    }

    @Test fun sidecarCarriesWhatTheComputerNeeds() {
        val start = ZonedDateTime.of(2026, 11, 26, 14, 5, 0, 0, ZoneId.of("Asia/Seoul")).toInstant().toEpochMilli()
        val r = recording("a").copy(startedAt = start, marksMs = listOf(760_000), eventTitle = "주간 회의", fileName = "2026-11-26 1405 주간 회의.m4a")
        val o = Json.parseToJsonElement(Sidecar.json(r, "Phone X", "0.1.0")).jsonObject
        assertEquals("2026-11-26T14:05:00+09:00", o["started_at"]!!.jsonPrimitive.content)
        assertEquals(760_000L, o["marks_ms"]!!.jsonArray[0].jsonPrimitive.content.toLong())
        assertEquals("주간 회의", o["event_title"]!!.jsonPrimitive.content)
        assertEquals("00", o["sha256"]!!.jsonPrimitive.content)
        assertEquals("2026-11-26 1405 주간 회의.json", Sidecar.fileName(r.fileName))
    }
}
