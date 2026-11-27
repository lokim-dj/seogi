package io.github.lokimdj.seogi.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull

/**
 * Changes made on either side, shared through the meeting's Drive folder
 * (issue #67; the computer's side is `src-tauri/src/seogi/edits.rs`, and
 * both decide the same way). Each device writes only its own file,
 * `고친것-폰.json` or `고친것-컴퓨터.json`, with its latest change of each
 * thing and when it was made. For every thing changed on either side the
 * later change stands (at the very same moment, the computer's); the other,
 * when it says something else, stays as a faint line the user can take
 * instead. Nothing is dropped without a trace.
 *
 * Keys: `done:<task id>` (true/false), `line:<start ms>` (a line's words),
 * `speaker:<id>` (a speaker's name).
 */
@Serializable
data class Edit(val key: String, val value: JsonElement, @SerialName("at_ms") val atMs: Long)

@Serializable
data class EditsFile(
    val format: Int = 1,
    val device: String = "",
    val meeting: String = "",
    val edits: List<Edit> = emptyList(),
) {
    /** The latest change of each key. */
    fun byKey(): Map<String, Edit> {
        val out = LinkedHashMap<String, Edit>()
        edits.forEach { e -> if ((out[e.key]?.atMs ?: Long.MIN_VALUE) <= e.atMs) out[e.key] = e }
        return out
    }

    companion object {
        private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }
        fun parse(text: String): EditsFile? = runCatching { json.decodeFromString(serializer(), text) }.getOrNull()
        fun phone(meeting: String, edits: Collection<Edit>) =
            json.encodeToString(serializer(), EditsFile(1, "phone", meeting, edits.sortedBy { it.key }))
    }
}

/** A change that did not stand and says something else than the one that did. */
data class OtherEdit(val key: String, val value: JsonElement, val fromComputer: Boolean)

object Edits {
    const val PHONE_FILE = "고친것-폰.json"
    const val COMPUTER_FILE = "고친것-컴퓨터.json"

    fun doneKey(taskId: String) = "done:$taskId"
    fun lineKey(startMs: Long) = "line:$startMs"
    fun speakerKey(id: Int) = "speaker:$id"

    /** For each key, the change that stands, and the others that said something else. */
    fun merge(computer: Map<String, Edit>, phone: Map<String, Edit>): Pair<Map<String, Edit>, List<OtherEdit>> {
        val winners = LinkedHashMap<String, Edit>()
        val losers = mutableListOf<OtherEdit>()
        (computer.keys + phone.keys).toSortedSet().forEach { key ->
            val c = computer[key]
            val p = phone[key]
            when {
                c != null && p != null -> {
                    val phoneWins = p.atMs > c.atMs
                    val win = if (phoneWins) p else c
                    val lose = if (phoneWins) c else p
                    if (win.value != lose.value) losers += OtherEdit(key, lose.value, fromComputer = phoneWins)
                    winners[key] = win
                }
                c != null -> winners[key] = c
                p != null -> winners[key] = p
            }
        }
        return winners to losers
    }

    /**
     * The minutes as they stand with every change made on either side,
     * also those the computer has not written into `회의.json` yet.
     */
    fun overlay(doc: MeetingDoc, winners: Map<String, Edit>): MeetingDoc {
        if (winners.isEmpty()) return doc
        fun done(e: MeetingDoc.Entry) =
            winners[doneKey(e.id)]?.value?.let { (it as? JsonPrimitive)?.booleanOrNull }?.let { e.copy(done = it) } ?: e
        fun text(v: JsonElement?) = (v as? JsonPrimitive)?.takeIf { it.isString }?.contentOrNull
        return doc.copy(
            tasks = doc.tasks.map(::done),
            sections = doc.sections.map { s -> if (s.kind == "tasks") s.copy(entries = s.entries.map(::done)) else s },
            lines = doc.lines.map { l -> text(winners[lineKey(l.startMs)]?.value)?.let { l.copy(text = it) } ?: l },
            speakers = doc.speakers.map { s ->
                text(winners[speakerKey(s.id)]?.value)?.takeIf { it.isNotBlank() }?.let { s.copy(name = it) } ?: s
            },
        )
    }
}
