package io.github.lokimdj.seogi.data

import java.time.LocalDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter

/** A recording file the app made in `Recordings/서기/`, as MediaStore lists it. */
data class PhoneFile(val uri: String, val name: String, val mime: String, val sizeBytes: Long, val durationMs: Long, val addedAt: Long)

/**
 * After the list had to come back from its backup, the backup may miss the
 * last change. The files on the phone are the truth: a file the list does not
 * know is added back (to upload; if it had been uploaded already, Drive may
 * get a second copy, which is better than a recording missing from the list),
 * and a line whose file is no longer on the phone is dropped. Lines still
 * being saved (no file yet) are left alone.
 */
object Reconcile {
    private val stamp = DateTimeFormatter.ofPattern("yyyy-MM-dd HHmm")

    fun merge(list: List<Recording>, files: List<PhoneFile>, zone: ZoneId): List<Recording> {
        val known = list.mapNotNull { it.mediaUri }.toSet()
        val onPhone = files.map { it.uri }.toSet()
        val kept = list.filter { it.mediaUri == null || it.mediaUri in onPhone }
        val added = files.filter { it.uri !in known }.map { fromFile(it, zone) }
        return (kept + added).sortedByDescending { it.startedAt }
    }

    /** `2026-11-26 1405 주간 회의.m4a` → start 14:05 that day, event title "주간 회의". */
    fun fromFile(f: PhoneFile, zone: ZoneId): Recording {
        val base = f.name.substringBeforeLast('.')
        val start = try {
            LocalDateTime.parse(base.take(15), stamp).atZone(zone)
        } catch (e: Exception) {
            java.time.Instant.ofEpochMilli(f.addedAt).atZone(zone)
        }
        val title = base.drop(16).trim().ifEmpty { null }
        return Recording(
            id = "rec-${start.toInstant().toEpochMilli()}-${f.uri.substringAfterLast('/')}",
            title = FileNames.displayTitle(start, title),
            eventTitle = title,
            startedAt = start.toInstant().toEpochMilli(),
            zone = zone.id,
            durationMs = f.durationMs,
            fileName = f.name,
            mimeType = f.mime,
            mediaUri = f.uri,
            sizeBytes = f.sizeBytes,
            sha256 = "",
            recovered = true,
        )
    }
}
