package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.data.FileNames
import org.junit.Assert.assertEquals
import org.junit.Test
import java.time.ZoneId
import java.time.ZonedDateTime

class FileNamesTest {
    private val start = ZonedDateTime.of(2026, 11, 26, 14, 5, 0, 0, ZoneId.of("Asia/Seoul"))

    @Test fun withEvent() = assertEquals("2026-11-26 1405 주간 회의", FileNames.baseName(start, "주간 회의"))

    @Test fun withoutEvent() {
        assertEquals("2026-11-26 1405", FileNames.baseName(start, null))
        assertEquals("2026-11-26 1405", FileNames.baseName(start, "  "))
    }

    @Test fun dropsCharactersFilesCannotHave() =
        assertEquals("2026-11-26 1405 a b c 계획", FileNames.baseName(start, "a/b: c?* <계획>|\n"))

    @Test fun longTitleIsCut() {
        val name = FileNames.baseName(start, "가".repeat(200))
        assertEquals("2026-11-26 1405 ".length + 80, name.length)
    }

    @Test fun noTrailingDot() = assertEquals("2026-11-26 1405 끝", FileNames.baseName(start, "끝..."))

    @Test fun displayKeepsTheTitleAsItIs() {
        assertEquals("a/b 회의", FileNames.displayTitle(start, " a/b 회의 "))
        assertEquals("14:05 녹음", FileNames.displayTitle(start, null))
    }
}
