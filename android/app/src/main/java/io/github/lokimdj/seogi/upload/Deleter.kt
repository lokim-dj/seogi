package io.github.lokimdj.seogi.upload

import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.data.RecordingStore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Deletes a recording the user chose to delete. Its copy in Drive goes to the
 * Drive trash (only files the app made, with the sidecar JSON; never a
 * permanent delete); an upload still under way is finished-and-trashed or
 * given up, so nothing is left behind; then the phone file, then the list
 * line. Nothing here runs on its own; only a user's confirmed "지우기" calls it.
 */
class Deleter(
    private val store: RecordingStore,
    private val drive: Drive,
    private val tokens: suspend () -> TokenResult,
    private val currentAccount: () -> String?,
    private val deleteOnPhone: (Recording) -> PhoneResult,
) {
    enum class PhoneResult { DELETED, ALREADY_GONE, FAILED }
    enum class DriveResult { TRASHED, LEFT_IN_DRIVE, NOT_UPLOADED, OTHER_ACCOUNT, FAILED }

    data class Result(
        val phoneDeleted: Boolean,
        val drive: DriveResult,
        /** The account the Drive copy is in, when it is not the one signed in now. */
        val otherAccount: String? = null,
        val listRemoved: Boolean = true,
    )

    suspend fun delete(r: Recording, trashInDrive: Boolean): Result {
        val now = currentAccount()?.ifEmpty { null }
        val driveResult: DriveResult
        var other: String? = null
        if (r.account != null && now != null && r.account != now) {
            // Uploaded with another Google account: this account cannot reach it.
            driveResult = if (r.driveFileId != null || r.sidecarFileId != null) DriveResult.OTHER_ACCOUNT else DriveResult.NOT_UPLOADED
            other = r.account
        } else {
            driveResult = drivePart(r, trashInDrive)
        }
        val phone = deleteOnPhone(r)
        val phoneDeleted = phone != PhoneResult.FAILED
        var listRemoved = false
        if (phoneDeleted) {
            listRemoved = try {
                store.remove(r.id)
                true
            } catch (e: Exception) {
                false
            }
        }
        return Result(phoneDeleted, driveResult, other, listRemoved)
    }

    private suspend fun drivePart(r: Recording, trashInDrive: Boolean): DriveResult {
        val ids = listOfNotNull(r.driveFileId, r.sidecarFileId).toMutableList()
        val session = r.sessionUri?.takeIf { r.driveFileId == null }
        if (ids.isEmpty() && session == null) return DriveResult.NOT_UPLOADED
        val token = (tokens() as? TokenResult.Ok)?.token ?: return if (trashInDrive) DriveResult.FAILED else DriveResult.LEFT_IN_DRIVE
        return try {
            withContext(Dispatchers.IO) {
                if (session != null) {
                    // An upload under way: if it just finished, that file counts too; if not, give it up.
                    when (val s = drive.status(token, session, r.sizeBytes)) {
                        is UploadStatus.Complete -> ids += s.fileId
                        is UploadStatus.Incomplete -> drive.cancelSession(session)
                        UploadStatus.Expired -> Unit
                    }
                }
                when {
                    ids.isEmpty() -> DriveResult.NOT_UPLOADED
                    !trashInDrive -> DriveResult.LEFT_IN_DRIVE
                    else -> {
                        ids.forEach { drive.trash(token, it) }
                        DriveResult.TRASHED
                    }
                }
            }
        } catch (e: Exception) {
            DriveResult.FAILED
        }
    }
}
