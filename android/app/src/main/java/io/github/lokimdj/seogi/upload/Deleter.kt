package io.github.lokimdj.seogi.upload

import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.data.RecordingStore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Deletes a recording the user chose to delete: its copy in Drive goes to the
 * Drive trash (only files the app made, with the sidecar JSON; never a
 * permanent delete), then the phone file, then the line in the list. Nothing
 * here runs on its own; only a user's confirmed "지우기" calls it.
 */
class Deleter(
    private val store: RecordingStore,
    private val drive: Drive,
    private val tokens: suspend () -> TokenResult,
    private val deleteOnPhone: (Recording) -> PhoneResult,
) {
    enum class PhoneResult { DELETED, ALREADY_GONE, FAILED }
    enum class DriveResult { TRASHED, LEFT_IN_DRIVE, NOT_UPLOADED, FAILED }
    data class Result(val phoneDeleted: Boolean, val drive: DriveResult)

    suspend fun delete(r: Recording, trashInDrive: Boolean): Result {
        val ids = listOfNotNull(r.driveFileId, r.sidecarFileId)
        val driveResult = when {
            ids.isEmpty() -> DriveResult.NOT_UPLOADED
            !trashInDrive -> DriveResult.LEFT_IN_DRIVE
            else -> trash(ids)
        }
        val phone = deleteOnPhone(r)
        val phoneDeleted = phone != PhoneResult.FAILED
        if (phoneDeleted) store.remove(r.id)
        return Result(phoneDeleted, driveResult)
    }

    private suspend fun trash(ids: List<String>): DriveResult {
        val token = (tokens() as? TokenResult.Ok)?.token ?: return DriveResult.FAILED
        return try {
            withContext(Dispatchers.IO) { ids.forEach { drive.trash(token, it) } }
            DriveResult.TRASHED
        } catch (e: Exception) {
            DriveResult.FAILED
        }
    }
}
