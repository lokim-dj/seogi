package io.github.lokimdj.seogi.record

import android.Manifest
import android.content.ContentUris
import android.content.Context
import android.content.pm.PackageManager
import android.provider.CalendarContract
import java.time.LocalDate
import java.time.ZoneId

/**
 * Reads today's events from the phone's calendar (CalendarContract), which
 * includes a Google calendar the phone already syncs. Nothing is sent anywhere.
 */
object CalendarReader {
    fun hasPermission(context: Context) =
        context.checkSelfPermission(Manifest.permission.READ_CALENDAR) == PackageManager.PERMISSION_GRANTED

    /** Today's timed events (no all-day, no cancelled, none I declined); empty without permission. */
    fun today(context: Context, now: Long = System.currentTimeMillis()): List<CalendarEvent> {
        if (!hasPermission(context)) return emptyList()
        val zone = ZoneId.systemDefault()
        val day = LocalDate.now(zone)
        val from = day.atStartOfDay(zone).toInstant().toEpochMilli()
        val to = day.plusDays(1).atStartOfDay(zone).toInstant().toEpochMilli()
        val uri = CalendarContract.Instances.CONTENT_URI.buildUpon().also {
            ContentUris.appendId(it, from)
            ContentUris.appendId(it, to)
        }.build()
        val cols = arrayOf(
            CalendarContract.Instances.TITLE,
            CalendarContract.Instances.BEGIN,
            CalendarContract.Instances.END,
            CalendarContract.Instances.ALL_DAY,
            CalendarContract.Instances.STATUS,
            CalendarContract.Instances.SELF_ATTENDEE_STATUS,
        )
        val out = mutableListOf<CalendarEvent>()
        try {
            context.contentResolver.query(uri, cols, null, null, CalendarContract.Instances.BEGIN)?.use { c ->
                while (c.moveToNext()) {
                    if (c.getInt(3) == 1) continue
                    if (c.getInt(4) == CalendarContract.Events.STATUS_CANCELED) continue
                    if (c.getInt(5) == CalendarContract.Attendees.ATTENDEE_STATUS_DECLINED) continue
                    out += CalendarEvent(c.getString(0).orEmpty().trim(), c.getLong(1), c.getLong(2))
                }
            }
        } catch (e: SecurityException) {
            return emptyList()
        }
        return out
    }
}
