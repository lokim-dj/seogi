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
    const val CH_RECORDING = "recording"
    const val CH_UPLOAD = "upload"
    const val ID_RECORDING = 1
    const val ID_UPLOAD = 2

    fun createChannels(context: Context) {
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CH_RECORDING, context.getString(R.string.channel_recording), NotificationManager.IMPORTANCE_LOW).apply {
                lockscreenVisibility = android.app.Notification.VISIBILITY_PUBLIC
                setShowBadge(false)
            },
        )
        nm.createNotificationChannel(
            NotificationChannel(CH_UPLOAD, context.getString(R.string.channel_upload), NotificationManager.IMPORTANCE_DEFAULT),
        )
    }

    fun openApp(context: Context): PendingIntent = PendingIntent.getActivity(
        context, 0,
        Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

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
