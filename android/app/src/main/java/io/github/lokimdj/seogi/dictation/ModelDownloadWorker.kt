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
                is ModelDownloader.Result.Unavailable -> {
                    android.util.Log.w("Seogi", "model download: HTTP ${r.code}")
                    Result.failure(workDataOf(KEY_UNAVAILABLE to true))
                }
            }
        } catch (e: IOException) {
            // Interrupted (the network, the server busy): later, from where it stopped.
            // After many tries in a row it stops and says so, rather than waiting for ever.
            if (runAttemptCount >= MAX_ATTEMPTS) Result.failure(workDataOf(KEY_UNAVAILABLE to true)) else Result.retry()
        }
    }

    companion object {
        const val NAME = "dictation-model"
        const val KEY_DONE = "done"
        const val KEY_WRONG = "wrong"
        const val KEY_UNAVAILABLE = "unavailable"
        /** Tag of a download that waits for Wi-Fi. */
        const val TAG_UNMETERED = "unmetered"
        private const val MAX_ATTEMPTS = 20

        /**
         * Starts the download. Begun on Wi-Fi ([unmetered]) it continues only on
         * Wi-Fi, so a dropped Wi-Fi never moves 850 MB onto mobile data; begun on
         * mobile data (the screen said so first) it goes on any connection.
         */
        fun start(context: Context, unmetered: Boolean) {
            val net = if (unmetered) NetworkType.UNMETERED else NetworkType.CONNECTED
            val req = OneTimeWorkRequestBuilder<ModelDownloadWorker>()
                .setConstraints(Constraints.Builder().setRequiredNetworkType(net).build())
                .apply { if (unmetered) addTag(TAG_UNMETERED) }
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
