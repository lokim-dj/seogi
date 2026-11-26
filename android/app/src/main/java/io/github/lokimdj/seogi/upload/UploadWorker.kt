package io.github.lokimdj.seogi.upload

import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.ParcelFileDescriptor
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import io.github.lokimdj.seogi.BuildConfig
import io.github.lokimdj.seogi.R
import io.github.lokimdj.seogi.SeogiApp
import io.github.lokimdj.seogi.data.Recording
import io.github.lokimdj.seogi.data.UploadError
import io.github.lokimdj.seogi.record.Notifications
import java.io.FileInputStream
import java.io.FileNotFoundException
import java.nio.ByteBuffer

/** Runs [Uploader] when the network allows; retries with growing waits. */
class UploadWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val app = applicationContext as SeogiApp
        val settings = app.settings
        val before = app.store.all.value.count { it.error == UploadError.MISSING_ON_PHONE }
        val uploader = Uploader(
            store = app.store,
            drive = DriveApi(),
            tokens = {
                // null: never signed in; "": signed in, address not known.
                settings.email.value?.let { GoogleAuth.token(app, it.ifEmpty { null }) } ?: TokenResult.NeedLogin
            },
            open = { openSource(app, it) },
            folder = object : Uploader.FolderCache {
                override var folderId: String?
                    get() = settings.driveFolderId
                    set(v) { settings.driveFolderId = v }
            },
            device = "${Build.MANUFACTURER} ${Build.MODEL}",
            appVersion = BuildConfig.VERSION_NAME,
        )
        val outcome = uploader.runAll()
        if (app.store.all.value.count { it.error == UploadError.MISSING_ON_PHONE } > before) {
            Notifications.uploadNeedsLook(app, app.getString(R.string.notice_missing))
        }
        return when (outcome) {
            Uploader.Outcome.DONE -> Result.success()
            Uploader.Outcome.RETRY -> Result.retry()
            Uploader.Outcome.NEED_LOGIN -> {
                Notifications.uploadNeedsLook(app, app.getString(R.string.notice_need_login))
                Result.failure()
            }
        }
    }

    private fun openSource(context: Context, r: Recording): AudioSource? {
        val uri = r.mediaUri ?: return null
        val pfd = try {
            context.contentResolver.openFileDescriptor(Uri.parse(uri), "r")
        } catch (e: FileNotFoundException) {
            null
        } catch (e: SecurityException) {
            null
        } ?: return null
        return PfdSource(pfd)
    }

    private class PfdSource(private val pfd: ParcelFileDescriptor) : AudioSource {
        private val stream = FileInputStream(pfd.fileDescriptor)
        override val size: Long = pfd.statSize
        override fun read(offset: Long, buf: ByteArray, at: Int, length: Int): Int =
            stream.channel.read(ByteBuffer.wrap(buf, at, length), offset)
        override fun close() {
            stream.close()
            pfd.close()
        }
    }
}
