package io.github.lokimdj.seogi.dictation

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * Brings the floating key back after Seogi is updated or the phone starts,
 * when the user left it on, so it does not wait for Seogi to be opened.
 * (When accessibility is on, [InsertService] being bound again does the same.)
 */
class KeyRestart : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            Intent.ACTION_MY_PACKAGE_REPLACED -> DictationService.resume(context, "updated")
            Intent.ACTION_BOOT_COMPLETED -> DictationService.resume(context, "phone started")
        }
    }
}
