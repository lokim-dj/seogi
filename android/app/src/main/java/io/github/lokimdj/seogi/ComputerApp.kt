package io.github.lokimdj.seogi

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context

/**
 * Seogi for the computer (Mac and Windows), which writes the minutes from the
 * phone's recordings: where to get it, opened from the last line of the
 * settings or copied to send to the computer.
 */
object ComputerApp {
    /** The latest release, with the Mac and Windows installers. The one place this address is kept. */
    const val DOWNLOAD_URL = "https://github.com/lokim-dj/seogi/releases/latest"

    /** Puts the address on the clipboard (Android shows that it was copied). */
    fun copyLink(context: Context) {
        context.getSystemService(ClipboardManager::class.java)
            ?.setPrimaryClip(ClipData.newPlainText(context.getString(R.string.set_computer_app), DOWNLOAD_URL))
    }
}
