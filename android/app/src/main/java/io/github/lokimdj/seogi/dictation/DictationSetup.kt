package io.github.lokimdj.seogi.dictation

import android.content.ComponentName
import android.content.Context
import android.provider.Settings
import java.io.File

/**
 * The few things dictation remembers, in small files rather than shared
 * preferences: the floating key runs in its own process (`:dictation`, so the
 * speech engine can never take a meeting recording down with it), and both
 * processes read these fresh each time.
 */
object DictationPrefs {
    private fun dir(context: Context) = File(context.filesDir, "dictation").apply { mkdirs() }
    private fun read(context: Context, name: String): String? =
        try { File(dir(context), name).takeIf { it.exists() }?.readText() } catch (e: Exception) { null }

    private fun write(context: Context, name: String, value: String?) {
        try {
            val f = File(dir(context), name)
            if (value == null) {
                f.delete()
            } else {
                val tmp = File(dir(context), "$name.tmp")
                tmp.writeText(value)
                tmp.renameTo(f)
            }
        } catch (_: Exception) {}
    }

    /** The user turned the floating key on (it comes back when Seogi is opened). */
    fun isOn(context: Context) = read(context, "on") != null
    fun setOn(context: Context, on: Boolean) = write(context, "on", if (on) "1" else null)

    /** The user chose to go without the accessibility step: text is copied, not typed. */
    fun copyOnly(context: Context) = read(context, "copy-only") != null
    fun setCopyOnly(context: Context, v: Boolean) {
        write(context, "copy-only", if (v) "1" else null)
        // Choosing copy only is choosing to go without accessibility: its going off is no drop (#98).
        if (v) setA11yWasOn(context, false)
    }

    /** The accessibility service was connected once and the user has not chosen copy only since (#98). */
    fun a11yWasOn(context: Context) = read(context, "a11y-was-on") != null
    fun setA11yWasOn(context: Context, v: Boolean) = write(context, "a11y-was-on", if (v) "1" else null)

    /** Since when (wall clock ms) accessibility has been on in the settings and not bound. */
    fun a11yUnboundSince(context: Context) = read(context, "a11y-unbound-since")?.trim()?.toLongOrNull()
    fun setA11yUnboundSince(context: Context, at: Long?) = write(context, "a11y-unbound-since", at?.toString())

    /** The connection's status as last seen, so a change is noted once in the drop log. */
    fun a11yLastStatus(context: Context) =
        read(context, "a11y-status")?.trim()?.let { s -> A11yStatus.entries.firstOrNull { it.name == s } }
    fun setA11yLastStatus(context: Context, s: A11yStatus) = write(context, "a11y-status", s.name)

    /** The newest process end already looked at for the drop log (wall clock ms). */
    fun exitsSeenUpTo(context: Context) = read(context, "exits-seen")?.trim()?.toLongOrNull() ?: 0L
    fun setExitsSeenUpTo(context: Context, at: Long) = write(context, "exits-seen", at.toString())

    /** The drop log's lines ([A11yLog]). */
    fun a11yLog(context: Context): List<String> = read(context, "a11y-log")?.lines()?.filter { it.isNotBlank() }.orEmpty()
    fun setA11yLog(context: Context, lines: List<String>) = write(context, "a11y-log", lines.joinToString("\n"))

    /** Where the floating key was last left. */
    fun place(context: Context) = BubblePlace.decode(read(context, "place"))
    fun setPlace(context: Context, p: BubblePlace) = write(context, "place", p.encode())
}

/** The steps of turning dictation on, one screen each, in this order. */
enum class SetupStep { MIC, OVERLAY, ACCESSIBILITY, MODEL, READY }

/** What the phone has granted and holds, read when the screen comes back. */
data class SetupState(
    val mic: Boolean,
    val notifications: Boolean,
    val overlay: Boolean,
    val accessibility: Boolean,
    val copyOnly: Boolean,
    val modelReady: Boolean,
) {
    /** The steps still to do; empty when everything is in place. */
    fun missing(): List<SetupStep> = buildList {
        if (!mic || !notifications) add(SetupStep.MIC)
        if (!overlay) add(SetupStep.OVERLAY)
        if (!accessibility && !copyOnly) add(SetupStep.ACCESSIBILITY)
        if (!modelReady) add(SetupStep.MODEL)
    }

    /** The screen to show: the first step not done, or the switch when all are. */
    fun current(): SetupStep = missing().firstOrNull() ?: SetupStep.READY

    /** "n / total" for the step on screen, counting the steps this phone needed at all. */
    fun position(step: SetupStep): Pair<Int, Int> {
        val all = SetupStep.entries.filter { it != SetupStep.READY }
        return Pair(all.indexOf(step) + 1, all.size)
    }
}

object Accessibility {
    /** True when Android's list of switched-on accessibility services has [service]. */
    fun enabledIn(setting: String?, service: ComponentName): Boolean =
        setting.orEmpty().split(':').any { ComponentName.unflattenFromString(it.trim()) == service }

    fun isOn(context: Context): Boolean = enabledIn(
        Settings.Secure.getString(context.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES),
        ComponentName(context, InsertService::class.java),
    )
}

object MeetingCheck {
    /** A stream written within this long is a recording under way, not one a crash left behind. */
    const val FRESH_MS = 15_000L

    /**
     * True when Seogi is recording a meeting now: something on the phone is
     * capturing sound ([anyCapture]; Android does not say which app) and
     * Seogi's own recording stream in [sessionDir] was written moments ago.
     * Another app's call or recorder alone is not a meeting.
     */
    fun recording(sessionDir: File, now: Long, anyCapture: Boolean): Boolean {
        if (!anyCapture) return false
        val streams = sessionDir.listFiles { f -> f.name.endsWith(".aac") }.orEmpty()
        return streams.any { now - it.lastModified() in 0..FRESH_MS }
    }
}
