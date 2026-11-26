package io.github.lokimdj.seogi.data

import kotlinx.serialization.Serializable

/** Where a recording is on its way to Drive. */
@Serializable
enum class UploadState { WAITING, UPLOADING, UPLOADED, FAILED }

/** Why an upload failed; [retriesByItself] says whether the queue tries again without the user. */
@Serializable
enum class UploadError(val retriesByItself: Boolean) {
    OFFLINE(true),
    SERVER(true),
    NEED_LOGIN(false),
    MISSING_ON_PHONE(false),
}

/**
 * One finished recording. The audio itself lives in MediaStore
 * (`Recordings/서기/`) and is never changed after it was saved.
 */
@Serializable
data class Recording(
    val id: String,
    val title: String,
    val eventTitle: String? = null,
    val startedAt: Long,
    val zone: String,
    val durationMs: Long,
    val marksMs: List<Long> = emptyList(),
    val fileName: String,
    val mimeType: String,
    val mediaUri: String? = null,
    val sizeBytes: Long,
    val sha256: String,
    val recovered: Boolean = false,
    val upload: UploadState = UploadState.WAITING,
    val progress: Float = 0f,
    val error: UploadError? = null,
    val sidecarFileId: String? = null,
    val sessionUri: String? = null,
    val driveFileId: String? = null,
    /** The Google account the Drive copy went to; deleting later checks it is still the same. */
    val account: String? = null,
) {
    /** Waiting, half done, or failed for a reason the queue handles by itself. */
    val needsUpload: Boolean
        get() = when (upload) {
            UploadState.WAITING, UploadState.UPLOADING -> true
            UploadState.FAILED -> error?.retriesByItself ?: true
            UploadState.UPLOADED -> false
        }
}
