package io.github.lokimdj.seogi

import android.app.PendingIntent
import android.content.Intent
import androidx.test.core.app.ApplicationProvider
import io.github.lokimdj.seogi.record.Notifications
import io.github.lokimdj.seogi.ui.OpenSourceParts
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File

@RunWith(RobolectricTestRunner::class)
@Config(application = android.app.Application::class, sdk = [34])
class AndroidPartsTest {
    private val context = ApplicationProvider.getApplicationContext<android.app.Application>()

    /** Review 4: the lock screen shows that it records, not what the meeting is called. */
    @Test fun lockScreenHidesTheMeetingTitle() {
        Notifications.createChannels(context)
        val pi = PendingIntent.getBroadcast(context, 0, Intent("x"), PendingIntent.FLAG_IMMUTABLE)
        val n = Notifications.recordingWithLockScreen(context, "고객 2차 상담", "녹음 중", 0L, pi, pi)
        assertEquals(android.app.Notification.VISIBILITY_PRIVATE, n.visibility)
        val pub = n.publicVersion
        assertEquals("서기", pub.extras.getCharSequence(android.app.Notification.EXTRA_TITLE).toString())
        assertTrue(pub.extras.toString().contains("고객").not())
        assertEquals(2, pub.actions.size)
    }

    /** Review 7: every part names a licence whose full text ships in the app. */
    @Test fun everyLicenceTextIsInTheApp() {
        OpenSourceParts.forEach { part ->
            val text = context.resources.openRawResource(part.licenseRes).bufferedReader().use { it.readText() }
            assertTrue(part.name, text.length > 3000)
        }
        val read = { res: Int -> context.resources.openRawResource(res).bufferedReader().use { it.readText() } }
        assertTrue(read(R.raw.license_apache_2_0).contains("Apache License"))
        assertTrue(read(R.raw.license_android_sdk).contains("Android Software Development Kit License Agreement"))
        assertTrue(read(R.raw.license_ofl_gmarket_sans).contains("SIL Open Font License"))
    }

    /** Minor: nothing of the app's own storage leaves through backup or a phone transfer. */
    @Test fun backupAndTransferExcludeEverything() {
        val manifest = File("src/main/AndroidManifest.xml").readText()
        assertTrue(manifest.contains("android:dataExtractionRules=\"@xml/data_extraction_rules\""))
        val rules = File("src/main/res/xml/data_extraction_rules.xml").readText()
        listOf("cloud-backup", "device-transfer").forEach { section ->
            val body = rules.substringAfter("<$section>").substringBefore("</$section>")
            listOf("root", "file", "database", "sharedpref", "external").forEach { d ->
                assertTrue("$section $d", body.contains("domain=\"$d\""))
            }
        }
    }
}
