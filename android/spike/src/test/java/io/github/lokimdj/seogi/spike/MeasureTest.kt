package io.github.lokimdj.seogi.spike

import org.junit.Assert.assertEquals
import org.junit.Test

class MeasureTest {
    @Test fun cerIgnoresSpacesAndPunctuation() {
        assertEquals(0.0, cer("도착하면 문자 한 통만 남겨 주세요.", "도착하면 문자 한통만 남겨주세요"), 1e-9)
    }

    @Test fun cerCountsWrongSyllables() {
        // 10 letters in the reference, one swapped, one dropped.
        assertEquals(0.2, cer("오늘은비가많이온다니까", "오늘은비가만이온다니"), 0.1)
        assertEquals(2.0 / 11, cer("오늘은비가많이온다니까", "오늘은비가만이온다니"), 1e-9)
    }

    @Test fun emptyHypothesisIsAllWrong() {
        assertEquals(1.0, cer("가나다", ""), 1e-9)
    }

    @Test fun percentiles() {
        val v = listOf(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0)
        assertEquals(5.5, percentile(v, 0.5), 1e-9)
        assertEquals(9.1, percentile(v, 0.9), 1e-9)
    }
}
