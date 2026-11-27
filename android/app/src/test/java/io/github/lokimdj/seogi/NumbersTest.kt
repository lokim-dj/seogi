package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.dictation.Numbers
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * The phone writes spoken numbers exactly as the computer does: the same
 * sentences the desktop's numbers.rs is tested with, read from the one file
 * both sides share.
 */
class NumbersTest {
    private val cases by lazy {
        val file = File("../../src-tauri/src/seogi/numbers.cases.json")
        assertTrue("cases at ${file.absolutePath}", file.exists())
        Json.parseToJsonElement(file.readText()).jsonObject
    }

    @Test fun writesNumbersBeforeUnitsAsDigits() {
        val changed = cases["changed"]!!.jsonArray.map { it.jsonObject }
        assertTrue("the shared cases did not load", changed.size > 50)
        val wrong = changed.mapNotNull {
            val said = it["said"]!!.jsonPrimitive.content
            val want = it["want"]!!.jsonPrimitive.content
            val got = Numbers.toDigits(said)
            if (got == want) null else "$said -> $got (want $want)"
        }
        assertTrue(wrong.joinToString("\n"), wrong.isEmpty())
    }

    @Test fun leavesWordsThatOnlyLookLikeNumbers() {
        val kept = cases["kept"]!!.jsonArray.map { it.jsonPrimitive.content }
        assertTrue("the shared cases did not load", kept.size > 100)
        val wrong = kept.mapNotNull { said -> Numbers.toDigits(said).let { if (it == said) null else "$said -> $it" } }
        assertTrue(wrong.joinToString("\n"), wrong.isEmpty())
    }

    @Test fun plainTextIsReturnedAsIs() {
        assertEquals("", Numbers.toDigits(""))
        assertEquals("   ", Numbers.toDigits("   "))
        assertEquals("Hello", Numbers.toDigits("Hello"))
        assertEquals("가나다", Numbers.toDigits("가나다"))
    }

    @Test fun punctuationStaysWhereItWas() {
        assertEquals("네, 3시 반에 봬요! 20% 할인이죠?", Numbers.toDigits("네, 세 시 반에 봬요! 이십 프로 할인이죠?"))
    }

    @Test fun longRunsOfNumberWordsStayQuick() {
        val long = "천 ".repeat(200_000)
        val started = System.nanoTime()
        assertEquals(long, Numbers.toDigits(long))
        assertTrue(System.nanoTime() - started < 2_000_000_000L)
    }
}
