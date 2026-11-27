package io.github.lokimdj.seogi.data

/**
 * One line of the phone's meeting list: a recording made here, a meeting in
 * the shared Drive folder, or both (a recording made here whose minutes the
 * computer wrote).
 */
data class ListEntry(
    val recording: Recording?,
    val meeting: DriveMeeting?,
) {
    val key: String get() = recording?.id ?: "m:${meeting!!.id}"
    val title: String get() = meeting?.title ?: recording!!.title
    val startedAt: Long get() = recording?.startedAt ?: meeting!!.startedAtMs
    val durationMs: Long get() = recording?.durationMs ?: meeting!!.durationMs
}

object MeetingList {
    /**
     * Every recording here and every meeting in Drive, newest first. A
     * meeting recorded on this phone is matched with its recording by the id
     * each upload carries, or (for files uploaded before ids) by the file
     * name, and shows as one line.
     */
    fun merge(recordings: List<Recording>, meetings: List<DriveMeeting>): List<ListEntry> {
        val taken = mutableSetOf<String>()
        val byRecording = meetings.filter { it.phoneRecordingId != null }.associateBy { it.phoneRecordingId!! }
        val entries = recordings.map { r ->
            val m = byRecording[r.id]
                ?: meetings.firstOrNull { it.origin == "phone" && it.phoneRecordingId == null && it.audioName == r.fileName }
            if (m != null) taken += m.id
            ListEntry(r, m)
        }
        val alone = meetings.filter { it.id !in taken }.map { ListEntry(null, it) }
        return (entries + alone).sortedByDescending { it.startedAt }
    }
}
