package io.github.lokimdj.seogi.dictation

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.IOException
import java.util.concurrent.TimeUnit

/**
 * Downloads the speech model in the background, so leaving the screen does not
 * stop it. Interrupted (the network, the system's time limit) it comes back and
 * continues from the last byte.
 */
class ModelDownloadWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val ctx = applicationContext
        val downloader = ModelDownloader(OkHttpRange, DictationModel.URL, DictationModel.SIZE, DictationModel.SHA256)
        var lastShown = -1
        try {
            val r = downloader.download(
                DictationModel.partFile(ctx), DictationModel.file(ctx),
                onProgress = { have ->
                    val permille = (have * 1000 / DictationModel.SIZE).toInt()
                    if (permille != lastShown) {
                        lastShown = permille
                        runBlocking { setProgress(workDataOf(KEY_DONE to have)) }
                    }
                },
                stopped = { isStopped },
            )
            when (r) {
                ModelDownloader.Result.Done -> Result.success()
                ModelDownloader.Result.Stopped -> Result.retry()
                is ModelDownloader.Result.Wrong -> {
                    android.util.Log.w("Seogi", "model download: ${r.why}")
                    Result.failure(workDataOf(KEY_WRONG to true))
                }
            }
        } catch (e: IOException) {
            Result.retry()
        }
    }

    companion object {
        const val NAME = "dictation-model"
        const val KEY_DONE = "done"
        const val KEY_WRONG = "wrong"

        fun start(context: Context) {
            val req = OneTimeWorkRequestBuilder<ModelDownloadWorker>()
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
                .build()
            WorkManager.getInstance(context).enqueueUniqueWork(NAME, ExistingWorkPolicy.KEEP, req)
        }

        /** Stops the download; what came so far is kept for the next start. */
        fun stop(context: Context) {
            WorkManager.getInstance(context).cancelUniqueWork(NAME)
        }
    }
}

/** [RangeFetch] on OkHttp. Hugging Face answers with a redirect to its file store, which is followed. */
object OkHttpRange : RangeFetch {
    private val client = OkHttpClient.Builder()
        .connectTimeout(20, TimeUnit.SECONDS)
        .readTimeout(60, TimeUnit.SECONDS)
        .build()

    override fun get(url: String, from: Long): Fetched {
        val req = Request.Builder().url(url).apply { if (from > 0) header("Range", "bytes=$from-") }.build()
        val res = client.newCall(req).execute()
        val body = res.body ?: run {
            res.close()
            throw IOException("no body")
        }
        return Fetched(res.code, body.byteStream())
    }
}
