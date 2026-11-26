package io.github.lokimdj.seogi.upload

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import io.github.lokimdj.seogi.SeogiApp
import java.util.concurrent.TimeUnit

/** One queue (`upload-queue`) that uploads everything waiting, one file after another. */
object UploadQueue {
    enum class Mode {
        /** On app start: leave a queue that is already there. */
        KEEP,
        /** A new recording or a retry: run after whatever is running now. */
        APPEND,
        /** The network setting changed: start over with the new rule (uploads resume). */
        REPLACE,
    }

    fun enqueue(context: Context, mode: Mode) {
        val app = context.applicationContext as SeogiApp
        val network = if (app.settings.wifiOnly.value) NetworkType.UNMETERED else NetworkType.CONNECTED
        val work = OneTimeWorkRequestBuilder<UploadWorker>()
            .setConstraints(Constraints.Builder().setRequiredNetworkType(network).build())
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
            .build()
        val policy = when (mode) {
            Mode.KEEP -> ExistingWorkPolicy.KEEP
            Mode.APPEND -> ExistingWorkPolicy.APPEND_OR_REPLACE
            Mode.REPLACE -> ExistingWorkPolicy.REPLACE
        }
        WorkManager.getInstance(context).enqueueUniqueWork(NAME, policy, work)
    }

    const val NAME = "upload-queue"
}
