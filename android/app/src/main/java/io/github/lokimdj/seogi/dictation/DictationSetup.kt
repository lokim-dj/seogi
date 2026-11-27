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
    fun setCopyOnly(context: Context, v: Boolean) = write(context, "copy-only", if (v) "1" else null)

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
