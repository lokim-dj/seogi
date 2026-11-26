package io.github.lokimdj.seogi.record

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import io.github.lokimdj.seogi.MainActivity
import io.github.lokimdj.seogi.R

/** The two notification channels: the running recording, and uploads that need a look. */
object Notifications {
    /** v2: the first channel let the lock screen show everything; this one leaves it to each notification. */
    const val CH_RECORDING = "recording-v2"
    private const val CH_RECORDING_OLD = "recording"
    const val CH_UPLOAD = "upload"
    const val ID_RECORDING = 1
    const val ID_UPLOAD = 2
    const val ID_NOTICE = 3

    fun createChannels(context: Context) {
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CH_RECORDING, context.getString(R.string.channel_recording), NotificationManager.IMPORTANCE_LOW).apply {
                setShowBadge(false)
            },
        )
        nm.deleteNotificationChannel(CH_RECORDING_OLD)
        nm.createNotificationChannel(
            NotificationChannel(CH_UPLOAD, context.getString(R.string.channel_upload), NotificationManager.IMPORTANCE_DEFAULT),
        )
    }

    /** The recording notification's common part; the caller adds the actions and who may see it. */
    fun recording(context: Context, title: String, text: String, startedAtWall: Long): NotificationCompat.Builder =
        NotificationCompat.Builder(context, CH_RECORDING)
            .setSmallIcon(R.drawable.ic_mark)
            .setColor(context.getColor(R.color.seal_red))
            .setContentTitle(title)
            .setContentText(text)
            .setWhen(startedAtWall)
            .setShowWhen(true)
            .setUsesChronometer(true)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_STATUS)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .setContentIntent(openApp(context))

    /**
     * The notification while recording: the meeting title in the shade, and
     * for the lock screen a version without it ("서기 · 녹음 중" and the time).
     */
    fun recordingWithLockScreen(
        context: Context,
        title: String,
        text: String,
        startedAtWall: Long,
        mark: PendingIntent,
        stop: PendingIntent,
    ): android.app.Notification {
        fun NotificationCompat.Builder.actions() = this
            .addAction(0, context.getString(R.string.action_mark), mark)
            .addAction(0, context.getString(R.string.action_stop), stop)
        val lockScreen = recording(
            context, context.getString(R.string.notif_public_title), context.getString(R.string.notif_recording), startedAtWall,
        ).actions().build()
        return recording(context, title, text, startedAtWall).actions()
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPublicVersion(lockScreen)
            .build()
    }

    fun openApp(context: Context): PendingIntent = PendingIntent.getActivity(
        context, 0,
        Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    /** A recording could not be saved; it is kept and the next start tries again. */
    fun saveFailed(context: Context) = notice(context, R.string.notice_save_failed_title, context.getString(R.string.notice_save_failed))

    /** The recording stopped on its own (no space, the microphone failed); what was recorded is saved. */
    fun recordingStopped(context: Context, why: Int) = notice(context, R.string.notice_recording_stopped, context.getString(why))

    private fun notice(context: Context, title: Int, text: String) {
        val n = NotificationCompat.Builder(context, CH_UPLOAD)
            .setSmallIcon(R.drawable.ic_mark)
            .setColor(context.getColor(R.color.seal_red))
            .setContentTitle(context.getString(title))
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setContentIntent(openApp(context))
            .setAutoCancel(true)
            .build()
        try {
            context.getSystemService(NotificationManager::class.java).notify(ID_NOTICE, n)
        } catch (e: SecurityException) {
            // Notifications not allowed: the home still shows it.
        }
    }

    /** One notice that an upload stopped and needs the user (login, or the file is gone). */
    fun uploadNeedsLook(context: Context, text: String) {
        val n = NotificationCompat.Builder(context, CH_UPLOAD)
            .setSmallIcon(R.drawable.ic_mark)
            .setColor(context.getColor(R.color.seal_red))
            .setContentTitle(context.getString(R.string.notice_upload_stopped))
            .setContentText(text)
            .setContentIntent(openApp(context))
            .setAutoCancel(true)
            .build()
        try {
            context.getSystemService(NotificationManager::class.java).notify(ID_UPLOAD, n)
        } catch (e: SecurityException) {
            // Notifications not allowed: the home list still shows it in red.
        }
    }
}
