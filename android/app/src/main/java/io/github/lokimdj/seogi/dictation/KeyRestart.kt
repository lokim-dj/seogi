package io.github.lokimdj.seogi.dictation

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * Brings the floating key back after Seogi is updated or the phone starts,
 * when the user left it on, so it does not wait for Seogi to be opened.
 * (When accessibility is on, Android binds [InsertService] again, and that
 * brings the key back without a notification.)
 */
class KeyRestart : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            Intent.ACTION_MY_PACKAGE_REPLACED -> DictationService.resume(context, "updated")
            Intent.ACTION_BOOT_COMPLETED -> DictationService.resume(context, "phone started")
        }
    }
}
