package io.github.lokimdj.seogi

import android.content.ClipboardManager
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(application = android.app.Application::class, sdk = [34])
class ComputerAppTest {
    private val context = ApplicationProvider.getApplicationContext<android.app.Application>()

    /** The latest release always has the Mac and Windows installers; a fixed version would go stale. */
    @Test fun theLinkIsTheLatestRelease() {
        assertTrue(ComputerApp.DOWNLOAD_URL.startsWith("https://"))
        assertTrue(ComputerApp.DOWNLOAD_URL.endsWith("/releases/latest"))
    }

    @Test fun copyingPutsTheLinkOnTheClipboard() {
        ComputerApp.copyLink(context)
        val clip = context.getSystemService(ClipboardManager::class.java).primaryClip!!
        assertEquals(ComputerApp.DOWNLOAD_URL, clip.getItemAt(0).text.toString())
        assertEquals(context.getString(R.string.set_computer_app), clip.description.label.toString())
    }
}
