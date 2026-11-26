package io.github.lokimdj.seogi.upload

import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.data.RecordingStore
import io.github.lokimdj.seogi.data.Sidecar
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
    /** The `서기 폰 녹음` folder uploads went to, when known. */
    private val folderId: () -> String? = { null },
) {
    enum class PhoneResult { DELETED, ALREADY_GONE, FAILED }
    enum class DriveResult {
        TRASHED,
        LEFT_IN_DRIVE,
        NOT_UPLOADED,
        OTHER_ACCOUNT,
        /** Drive does not know the file in this account: gone already, or in an account we cannot tell. */
        NOT_FOUND,
        FAILED,
    }

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

    private suspend fun drivePart(given: Recording, trashInDrive: Boolean): DriveResult {
        // The list line may know files the copy taken at "지우기" did not.
        val r = store.get(given.id) ?: given
        val ids = listOfNotNull(r.driveFileId, r.sidecarFileId).toMutableList()
        val session = r.sessionUri?.takeIf { r.driveFileId == null }
        val folder = folderId()?.takeIf { trashInDrive }
        val known = ids.isNotEmpty() || session != null
        if (!known && folder == null) return DriveResult.NOT_UPLOADED
        val token = (tokens() as? TokenResult.Ok)?.token
            ?: return if (!known) DriveResult.NOT_UPLOADED else if (trashInDrive) DriveResult.FAILED else DriveResult.LEFT_IN_DRIVE
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
                if (folder != null) {
                    // Every file in the folder that belongs to this recording, also ones the
                    // list lost track of (a second sidecar from a retry), goes with it.
                    val found = try {
                        drive.findByName(token, folder, listOf(r.fileName, Sidecar.fileName(r.fileName)))
                    } catch (e: Exception) {
                        // Nothing was known to be in Drive: a failed look does not make it a failure.
                        if (known) throw e else emptyList()
                    }
                    found.filter { belongs(it, r) }.forEach { if (it.id !in ids) ids += it.id }
                }
                when {
                    ids.isEmpty() -> DriveResult.NOT_UPLOADED
                    !trashInDrive -> DriveResult.LEFT_IN_DRIVE
                    else -> {
                        // Every file must really have gone to the trash to say so.
                        val all = ids.map { drive.trash(token, it) }.all { it }
                        if (all) DriveResult.TRASHED else DriveResult.NOT_FOUND
                    }
                }
            }
        } catch (e: Exception) {
            DriveResult.FAILED
        }
    }

    /**
     * A file found by name is this recording's when it carries this
     * recording's id, or (uploaded before files carried one) when it is the
     * sidecar made after the recording ended, or audio whose SHA-256 Drive
     * reports as this recording's.
     */
    private fun belongs(f: DriveFile, r: Recording): Boolean {
        if (f.recordingId != null) return f.recordingId == r.id
        return when (f.name) {
            Sidecar.fileName(r.fileName) -> f.isOf(r)
            r.fileName -> r.sha256.isNotEmpty() && f.sha256.equals(r.sha256, ignoreCase = true)
            else -> false
        }
    }

    companion object {
        /**
         * Whether the delete question offers the Drive trash: when the list
         * knows a Drive file, or an upload went to a known folder, which may
         * hold a file the list lost track of (a sidecar whose answer was lost).
         */
        fun offerDriveTrash(r: Recording, folderId: String?): Boolean =
            r.driveFileId != null || r.sidecarFileId != null || r.sessionUri != null || folderId != null
    }
}
