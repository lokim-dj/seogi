package io.github.lokimdj.seogi.dictation

import android.accessibilityservice.AccessibilityServiceInfo
import android.app.Activity
import android.app.ActivityManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.provider.Settings
import android.util.Log
import android.view.accessibility.AccessibilityManager
import androidx.core.app.NotificationCompat
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.Worker
import androidx.work.WorkerParameters
import io.github.lokimdj.seogi.R
import java.util.concurrent.TimeUnit

/**
 * Watches Seogi's accessibility connection (#98) from any of its processes:
 * reads where it stands ([A11yHealth]), notes drops in the short log
 * ([A11yLog]), and offers the one tap that brings it back.
 *
 * "Bound" is read from Android: [AccessibilityManager.getEnabledAccessibilityServiceList]
 * lists the services Android has connected now, not the ones switched on in
 * the settings, so a service on in the settings and missing there is one
 * Android dropped.
 */
object A11yWatch {
    private const val TAG = "SeogiDictation"

    /** The notification channel the key's notification uses too. */
    const val CHANNEL = "dictation"

    /** The one-tap notification posted when the key's own notification is not up ([A11yHealth.promptNotification]). */
    const val ID_PROMPT = 12

    private const val WORK = "a11y-watch"
    private const val WORK_AGAIN = "a11y-watch-again"
    private const val ACTION_DETAILS = "android.settings.ACCESSIBILITY_DETAILS_SETTINGS"

    fun component(context: Context) = ComponentName(context, InsertService::class.java)

    /** True when Android has Seogi's accessibility service connected now. */
    fun isBound(context: Context): Boolean = try {
        val me = component(context)
        context.getSystemService(AccessibilityManager::class.java)
            ?.getEnabledAccessibilityServiceList(AccessibilityServiceInfo.FEEDBACK_ALL_MASK)
            .orEmpty()
            .any { info -> info.resolveInfo?.serviceInfo?.let { ComponentName(it.packageName, it.name) } == me }
    } catch (e: Exception) {
        false
    }

    /**
     * Where the connection stands now, noting a change in the log. [bound]
     * is passed by the service itself, which knows better than the list in
     * the moment it connects or goes.
     */
    fun look(context: Context, bound: Boolean = isBound(context), now: Long = System.currentTimeMillis()): A11yStatus =
        DictationPrefs.locked(context) { lookLocked(context, bound, now) }

    private fun lookLocked(context: Context, bound: Boolean, now: Long): A11yStatus {
        val enabled = Accessibility.isOn(context)
        val since = A11yHealth.unboundSince(DictationPrefs.a11yUnboundSince(context), enabled, bound, now)
        if (since != DictationPrefs.a11yUnboundSince(context)) DictationPrefs.setA11yUnboundSince(context, since)
        val status = A11yHealth.status(
            keyOn = DictationPrefs.isOn(context),
            enabled = enabled,
            bound = bound,
            wasOn = DictationPrefs.a11yWasOn(context),
            unboundForMs = since?.let { now - it },
        )
        val before = DictationPrefs.a11yLastStatus(context)
        if (before != status) {
            DictationPrefs.setA11yLastStatus(context, status)
            val kind = A11yHealth.note(before, status)
            if (kind != null) {
                // Android's reason for a process that ended goes in first, so the log reads in order.
                noteExitsLocked(context, now)
                addLocked(context, A11yLog.Entry(now, kind))
                Log.i(TAG, "accessibility: $before -> $status")
            }
        }
        return status
    }

    /** The service connected: remember it was on, note the way back, take the prompt down. */
    fun connected(context: Context) {
        DictationPrefs.locked(context) {
            DictationPrefs.setA11yWasOn(context, true)
            val now = System.currentTimeMillis()
            lookLocked(context, bound = true, now = now)
            noteExitsLocked(context, now)
        }
        cancelPrompt(context)
    }

    private fun addLocked(context: Context, e: A11yLog.Entry) {
        DictationPrefs.setA11yLog(context, A11yLog.appended(DictationPrefs.a11yLog(context), e))
    }

    /** The drop log, oldest first. */
    fun entries(context: Context): List<A11yLog.Entry> = DictationPrefs.a11yLog(context).mapNotNull(A11yLog::parse)

    /**
     * Notes the dictation processes that ended since last looked
     * ([ExitNote]): Android keeps their reasons (low memory, a crash, a
     * force stop) in [ActivityManager.getHistoricalProcessExitReasons].
     */
    fun noteExits(context: Context) {
        DictationPrefs.locked(context) { noteExitsLocked(context, System.currentTimeMillis()) }
    }

    private fun noteExitsLocked(context: Context, now: Long) {
        val seen = DictationPrefs.exitsSeenUpTo(context)
        if (seen == null) {
            // First look after this version came: older ends are not drops this log can explain.
            DictationPrefs.setExitsSeenUpTo(context, now)
            return
        }
        val exits = try {
            context.getSystemService(ActivityManager::class.java)
                ?.getHistoricalProcessExitReasons(context.packageName, 0, 16)
                .orEmpty()
                .map { Triple(it.timestamp, it.processName.orEmpty(), it.reason) }
        } catch (e: Exception) {
            return
        }
        // With the key off or accessibility never on, Android reaping an idle `:dictation` is no drop.
        val keyInUse = DictationPrefs.isOn(context) && DictationPrefs.a11yWasOn(context)
        for ((at, process, reason) in ExitNote.fresh(exits, seen, keyInUse)) {
            addLocked(context, A11yLog.Entry(at, A11yLog.Kind.EXIT, ExitNote.detail(process, reason)))
        }
        val newest = exits.maxOfOrNull { it.first } ?: return
        if (newest > seen) DictationPrefs.setExitsSeenUpTo(context, newest)
    }

    // --- the one tap ---

    /**
     * Android's page with Seogi's accessibility switch, or the accessibility
     * list where a phone does not offer that page.
     */
    fun openSettings(context: Context) {
        val intents = listOf(
            // Android's ACCESSIBILITY_DETAILS_SETTINGS (API 31+), not a public constant in the SDK.
            Intent(ACTION_DETAILS)
                .putExtra(Intent.EXTRA_COMPONENT_NAME, component(context).flattenToString()),
            Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS),
        )
        for (i in intents) {
            if (context !is Activity) i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            try {
                context.startActivity(i)
                return
            } catch (_: Exception) {}
        }
    }

    /** What a tap on a notification does: [A11yFixActivity], which opens [openSettings]. */
    fun fixIntent(context: Context): PendingIntent = PendingIntent.getActivity(
        context, 0, Intent(context, A11yFixActivity::class.java),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    fun channel(context: Context) {
        context.getSystemService(NotificationManager::class.java)?.createNotificationChannel(
            NotificationChannel(CHANNEL, context.getString(R.string.channel_dictation), NotificationManager.IMPORTANCE_LOW).apply {
                setShowBadge(false)
            },
        )
    }

    /** Whether the key's own notification is up (it then carries the prompt itself). */
    private fun keyNotificationShowing(context: Context): Boolean = try {
        context.getSystemService(NotificationManager::class.java)?.activeNotifications.orEmpty()
            .any { it.id == DictationService.ID_NOTIFICATION }
    } catch (e: Exception) {
        false
    }

    /**
     * Looks, and posts or takes down the one-tap notification: the
     * accessibility service going, the periodic check, Seogi's start. When
     * Android may still be bringing the service back, and this is the main
     * process (where WorkManager runs), looks again once the grace has passed.
     */
    fun check(context: Context, bound: Boolean = isBound(context), canSchedule: Boolean = true) {
        val status = look(context, bound)
        if (A11yHealth.promptNotification(status, keyNotificationShowing(context))) postPrompt(context) else cancelPrompt(context)
        if (status == A11yStatus.CONNECTING && canSchedule) {
            try {
                WorkManager.getInstance(context).enqueueUniqueWork(
                    WORK_AGAIN, ExistingWorkPolicy.REPLACE,
                    OneTimeWorkRequestBuilder<Check>().setInitialDelay(A11yHealth.GRACE_MS + 1_000, TimeUnit.MILLISECONDS).build(),
                )
            } catch (e: Exception) {
                Log.w(TAG, "could not schedule the accessibility check", e)
            }
        }
    }

    private fun postPrompt(context: Context) {
        channel(context)
        val n = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_dictation)
            .setContentTitle(context.getString(R.string.notif_a11y_dropped))
            .setContentText(context.getString(R.string.notif_a11y_fix))
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setAutoCancel(true)
            .setCategory(NotificationCompat.CATEGORY_STATUS)
            .setContentIntent(fixIntent(context))
            .build()
        try {
            context.getSystemService(NotificationManager::class.java)?.notify(ID_PROMPT, n)
        } catch (_: SecurityException) {}
    }

    fun cancelPrompt(context: Context) {
        try { context.getSystemService(NotificationManager::class.java)?.cancel(ID_PROMPT) } catch (_: Exception) {}
    }

    // --- the periodic check ---

    /**
     * While the key is on, a check every 15 minutes (Android's shortest):
     * when the key's whole process is gone, nothing in it can say so.
     */
    fun schedule(context: Context) {
        try {
            WorkManager.getInstance(context).enqueueUniquePeriodicWork(
                WORK, ExistingPeriodicWorkPolicy.KEEP,
                PeriodicWorkRequestBuilder<Check>(15, TimeUnit.MINUTES).build(),
            )
        } catch (e: Exception) {
            Log.w(TAG, "could not schedule the accessibility check", e)
        }
    }

    fun unschedule(context: Context) {
        try {
            WorkManager.getInstance(context).cancelUniqueWork(WORK)
            WorkManager.getInstance(context).cancelUniqueWork(WORK_AGAIN)
        } catch (_: Exception) {}
        cancelPrompt(context)
    }

    /** The periodic check ([schedule]), and the look again after the grace ([check]). */
    class Check(context: Context, params: WorkerParameters) : Worker(context, params) {
        override fun doWork(): Result {
            A11yWatch.check(applicationContext)
            return Result.success()
        }
    }
}

/**
 * Opens Android's page with Seogi's accessibility switch from a notification
 * and goes: a notification's tap cannot try one page and fall back to
 * another, an activity can ([A11yWatch.openSettings]).
 */
class A11yFixActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        A11yWatch.openSettings(this)
        finish()
    }
}
