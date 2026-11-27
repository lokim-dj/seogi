package io.github.lokimdj.seogi.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/**
 * `회의.json`, written by the computer in each meeting's Drive folder: what
 * the phone reads of a meeting (format in `docs/drive-folder.md`). Fields are
 * only ever added there, so unknown ones are ignored here and every field
 * has a default.
 */
@Serializable
data class MeetingDoc(
    val format: Int = 1,
    val id: String = "",
    val title: String = "",
    @SerialName("started_at_ms") val startedAtMs: Long = 0,
    @SerialName("duration_ms") val durationMs: Long = 0,
    val origin: String = "",
    val audio: Audio = Audio(),
    val kind: String = "",
    val summary: String = "",
    val tasks: List<Entry> = emptyList(),
    val story: List<StoryPart> = emptyList(),
    val sections: List<Section> = emptyList(),
    @SerialName("marks_ms") val marksMs: List<Long> = emptyList(),
    val speakers: List<Speaker> = emptyList(),
    val lines: List<Line> = emptyList(),
) {
    @Serializable
    data class Audio(val file: String = "", val sha256: String = "", @SerialName("in_folder") val inFolder: Boolean = false)

    @Serializable
    data class Source(val ms: Long = 0, val quote: String = "")

    /** A task, or a line of another section. */
    @Serializable
    data class Entry(
        val id: String = "",
        val text: String = "",
        val owner: String = "",
        val due: String = "",
        val done: Boolean = false,
        @SerialName("needs_check") val needsCheck: Boolean = false,
        val added: Boolean = false,
        val sources: List<Source> = emptyList(),
    )

    @Serializable
    data class StoryPart(val title: String = "", val text: String = "", val sources: List<Source> = emptyList())

    /** `kind`: `tasks`, `items` or `quotes`. */
    @Serializable
    data class Section(
        val key: String = "",
        val title: String = "",
        val kind: String = "",
        val entries: List<Entry> = emptyList(),
    )

    @Serializable
    data class Speaker(val id: Int = 0, val name: String = "")

    @Serializable
    data class Line(@SerialName("start_ms") val startMs: Long = 0, @SerialName("end_ms") val endMs: Long = 0, val speaker: Int? = null, val text: String = "")

    /** The sections that are not tasks: they go under "자세히". */
    val otherSections: List<Section> get() = sections.filter { it.kind != "tasks" && it.entries.isNotEmpty() }

    /** A speaker's name as the computer last wrote it; `화자 n` when it has none. */
    fun speakerName(id: Int?): String? =
        id?.let { n -> speakers.firstOrNull { it.id == n }?.name?.takeIf { it.isNotBlank() } ?: "화자 $n" }

    companion object {
        private val json = Json { ignoreUnknownKeys = true; coerceInputValues = true; isLenient = true }

        /** Null when the text is not a meeting the phone can read. */
        fun parse(text: String): MeetingDoc? =
            runCatching { json.decodeFromString(serializer(), text) }.getOrNull()?.takeIf { it.id.isNotEmpty() }
    }
}
