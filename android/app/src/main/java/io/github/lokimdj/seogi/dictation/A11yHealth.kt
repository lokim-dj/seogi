package io.github.lokimdj.seogi.dictation

import android.app.ApplicationExitInfo

/**
 * How Seogi's accessibility connection stands, as the floating key needs it
 * (#98). The user switches the service on once in Android's settings; Android
 * can drop it afterwards without a word: a crash marks it "not working" and
 * leaves it unbound although it is still on in the settings, and stopping the
 * app by force takes it out of the settings altogether. Seogi cannot switch it
 * back on itself, so it notices and offers the one tap that does.
 */
enum class A11yStatus {
    /** Bound: the key follows the text fields. */
    FINE,

    /** Nothing to say: the key is off, or the user never switched accessibility on (copy only). */
    NOT_NEEDED,

    /** On in the settings and not bound yet, for less than [A11yHealth.GRACE_MS]: Android may still be bringing it back. */
    CONNECTING,

    /** On in the settings, yet not bound for [A11yHealth.GRACE_MS] or more: Android gave up on it ("작동 안 함"). */
    DROPPED,

    /** It was on, and is no longer on in the settings. */
    TURNED_OFF,
}

/** The decisions about the accessibility connection. Pure, so it is tested on the computer. */
object A11yHealth {
    /**
     * How long "on but not bound" is let pass before it counts as dropped.
     * After its process is killed Android restarts a bound service with at
     * least 10 s between tries (ActiveServices, SERVICE_MIN_RESTART_TIME_BETWEEN),
     * and binding right after the phone starts takes a moment too.
     */
    const val GRACE_MS = 30_000L

    /**
     * Where the connection stands. [keyOn]: the user left the floating key on.
     * [enabled]: Seogi is in Android's list of switched-on services.
     * [bound]: Android has the service connected now. [wasOn]: it was
     * connected before and the user has not chosen copy only since.
     * [unboundForMs]: how long it has been on and not bound (null when not).
     */
    fun status(keyOn: Boolean, enabled: Boolean, bound: Boolean, wasOn: Boolean, unboundForMs: Long?): A11yStatus = when {
        bound -> A11yStatus.FINE
        !keyOn -> A11yStatus.NOT_NEEDED
        enabled -> if ((unboundForMs ?: 0L) >= GRACE_MS) A11yStatus.DROPPED else A11yStatus.CONNECTING
        wasOn -> A11yStatus.TURNED_OFF
        else -> A11yStatus.NOT_NEEDED
    }

    /** Whether the user must be asked for the one tap. */
    fun needsFix(s: A11yStatus): Boolean = s == A11yStatus.DROPPED || s == A11yStatus.TURNED_OFF

    /**
     * Since when the service has been on and not bound: [previous] kept,
     * [now] when it starts, null when it is bound or not on. A [previous]
     * in the future (the clock was set back) starts again from [now].
     */
    fun unboundSince(previous: Long?, enabled: Boolean, bound: Boolean, now: Long): Long? = when {
        bound || !enabled -> null
        previous == null || previous > now -> now
        else -> previous
    }

    /** What the drop log notes when the status goes from [before] to [now]; null for nothing. */
    fun note(before: A11yStatus?, now: A11yStatus): A11yLog.Kind? = when {
        before == now -> null
        now == A11yStatus.DROPPED -> A11yLog.Kind.DROPPED
        now == A11yStatus.TURNED_OFF -> A11yLog.Kind.TURNED_OFF
        now == A11yStatus.FINE && (before == A11yStatus.DROPPED || before == A11yStatus.TURNED_OFF) -> A11yLog.Kind.BACK
        else -> null
    }

    /**
     * Whether the separate one-tap notification shows: the connection needs
     * the fix and the key's own notification, which then says the same, is
     * not up.
     */
    fun promptNotification(s: A11yStatus, keyNotificationShowing: Boolean): Boolean = needsFix(s) && !keyNotificationShowing
}

/**
 * The short drop log, kept on the phone only so the next try on a phone can
 * show why the connection went: when, what was seen, and for a process that
 * ended, Android's reason. Never what was said or written.
 */
object A11yLog {
    enum class Kind {
        /** On in the settings, not bound. */
        DROPPED,

        /** Taken out of the settings. */
        TURNED_OFF,

        /** Connected again after a drop. */
        BACK,

        /** A dictation process ended; the detail says which and Android's reason ([ExitNote]). */
        EXIT,
    }

    data class Entry(val at: Long, val kind: Kind, val detail: String = "")

    /** Lines kept: the newest. */
    const val KEEP = 40

    private const val DETAIL_MAX = 40

    /** One line of the log file. The detail keeps only letters, digits and `:-_`. */
    fun line(e: Entry): String {
        val detail = e.detail.filter { it.isLetterOrDigit() || it in ":-_" }.take(DETAIL_MAX)
        return "${e.at}\t${e.kind.name}\t$detail"
    }

    /** The entry of a line, or null for a line that is not one. */
    fun parse(line: String): Entry? {
        val parts = line.split('\t')
        if (parts.size !in 2..3) return null
        val at = parts[0].toLongOrNull() ?: return null
        val kind = Kind.entries.firstOrNull { it.name == parts[1] } ?: return null
        return Entry(at, kind, parts.getOrElse(2) { "" })
    }

    /** [lines] with [e] added at the end, keeping the newest [keep]. */
    fun appended(lines: List<String>, e: Entry, keep: Int = KEEP): List<String> =
        (lines.filter { parse(it) != null } + line(e)).takeLast(keep)
}

/**
 * Which ended dictation processes go in the drop log, and in what words.
 * The key and the accessibility service live in `:dictation`, which should
 * never end while the service is on, so any end of it is noted. The speech
 * engine lives in `:engine` and is let go when idle, so only its crashes are.
 */
object ExitNote {
    const val DICTATION = ":dictation"
    const val ENGINE = ":engine"

    /** Whether the end of [process] (a full process name) for [reason] (ApplicationExitInfo) is noted. */
    fun worth(process: String, reason: Int): Boolean = when {
        process.endsWith(DICTATION) -> reason != ApplicationExitInfo.REASON_EXIT_SELF &&
            reason != ApplicationExitInfo.REASON_PACKAGE_UPDATED
        process.endsWith(ENGINE) -> reason in ENGINE_FAILURES
        else -> false
    }

    private val ENGINE_FAILURES = setOf(
        ApplicationExitInfo.REASON_CRASH,
        ApplicationExitInfo.REASON_CRASH_NATIVE,
        ApplicationExitInfo.REASON_ANR,
        ApplicationExitInfo.REASON_INITIALIZATION_FAILURE,
    )

    /** The detail for the log: which process, and the reason in one word ("dictation:low-memory"). */
    fun detail(process: String, reason: Int): String {
        val who = if (process.endsWith(ENGINE)) "engine" else "dictation"
        return "$who:${word(reason)}"
    }

    /** Android's reason in one word. */
    fun word(reason: Int): String = when (reason) {
        ApplicationExitInfo.REASON_LOW_MEMORY -> "low-memory"
        ApplicationExitInfo.REASON_CRASH -> "crash"
        ApplicationExitInfo.REASON_CRASH_NATIVE -> "native-crash"
        ApplicationExitInfo.REASON_ANR -> "anr"
        ApplicationExitInfo.REASON_USER_REQUESTED -> "force-stop"
        ApplicationExitInfo.REASON_SIGNALED -> "signaled"
        ApplicationExitInfo.REASON_EXCESSIVE_RESOURCE_USAGE -> "excessive-use"
        ApplicationExitInfo.REASON_DEPENDENCY_DIED -> "dependency-died"
        ApplicationExitInfo.REASON_INITIALIZATION_FAILURE -> "init-failure"
        ApplicationExitInfo.REASON_PERMISSION_CHANGE -> "permission-change"
        ApplicationExitInfo.REASON_FREEZER -> "freezer"
        ApplicationExitInfo.REASON_USER_STOPPED -> "user-stopped"
        else -> "other"
    }

    /**
     * Which exits to note: newer than [seenUpTo], worth it, oldest first.
     * Each is (time, process, reason). Ends of `:dictation` only while
     * [keyInUse] (the key on and accessibility once connected): otherwise
     * Android reaping the idle process is no drop.
     */
    fun fresh(exits: List<Triple<Long, String, Int>>, seenUpTo: Long, keyInUse: Boolean = true): List<Triple<Long, String, Int>> =
        exits.filter { it.first > seenUpTo && worth(it.second, it.third) && (keyInUse || !it.second.endsWith(DICTATION)) }
            .sortedBy { it.first }
}

/** What the key's notification says while it runs without accessibility. Pure, so it is tested. */
object KeyNotice {
    enum class Line {
        /** "접근성이 끊겼어요 · 눌러서 다시 켜기": a tap opens Seogi's accessibility switch. */
        FIX,

        /** The user hid the key. */
        HIDDEN,

        /** The key shows while the user types. */
        TYPING,

        /** The key is always up (copy only). */
        ALWAYS,
    }

    /** The drop comes first: a hidden key still needs its accessibility back. */
    fun line(hidden: Boolean, follows: Boolean, a11y: A11yStatus): Line = when {
        A11yHealth.needsFix(a11y) -> Line.FIX
        hidden -> Line.HIDDEN
        follows -> Line.TYPING
        else -> Line.ALWAYS
    }
}
