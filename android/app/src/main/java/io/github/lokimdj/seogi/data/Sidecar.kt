package io.github.lokimdj.seogi.data

import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

/**
 * The small JSON uploaded next to each recording, for the computer's Seogi:
 * when it started, how long, the important marks, the calendar title, the
 * phone, and the SHA-256 of the audio file.
 */
object Sidecar {
    fun fileName(audioFileName: String): String = audioFileName.substringBeforeLast('.') + ".json"

    fun json(r: Recording, device: String, appVersion: String): String = buildJsonObject {
        put("app", "seogi-android")
        put("app_version", appVersion)
        put("file", r.fileName)
        put(
            "started_at",
            DateTimeFormatter.ISO_OFFSET_DATE_TIME.format(Instant.ofEpochMilli(r.startedAt).atZone(ZoneId.of(r.zone))),
        )
        put("duration_ms", r.durationMs)
        putJsonArray("marks_ms") { r.marksMs.forEach { add(JsonPrimitive(it)) } }
        put("title", r.title)
        put("event_title", r.eventTitle)
        put("device", device)
        put("sha256", r.sha256)
        put("recovered", r.recovered)
    }.toString()
}
