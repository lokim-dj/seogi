package io.github.lokimdj.seogi

import android.content.ComponentName
import android.content.pm.PackageManager
import androidx.test.core.app.ApplicationProvider
import io.github.lokimdj.seogi.dictation.Accessibility
import io.github.lokimdj.seogi.dictation.BubblePlace
import io.github.lokimdj.seogi.dictation.DictationPrefs
import io.github.lokimdj.seogi.dictation.DictationService
import io.github.lokimdj.seogi.dictation.InsertService
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.After
import org.junit.Test
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(application = android.app.Application::class, sdk = [34])
class DictationAndroidTest {
    private val context = ApplicationProvider.getApplicationContext<android.app.Application>()
    private val service = ComponentName("io.github.lokimdj.seogi", "io.github.lokimdj.seogi.dictation.InsertService")

    @Test fun accessibilityIsReadFromAndroidsList() {
        assertTrue(Accessibility.enabledIn("com.example/.Reader:io.github.lokimdj.seogi/.dictation.InsertService", service))
        assertTrue(Accessibility.enabledIn("io.github.lokimdj.seogi/io.github.lokimdj.seogi.dictation.InsertService", service))
        assertFalse(Accessibility.enabledIn("io.github.lokimdj.seogi.other/.dictation.InsertService", service))
        assertFalse(Accessibility.enabledIn(null, service))
        assertFalse(Accessibility.enabledIn("", service))
    }

    @Test fun prefsAreKeptInFiles() {
        assertFalse(DictationPrefs.isOn(context))
        DictationPrefs.setOn(context, true)
        assertTrue(DictationPrefs.isOn(context))
        DictationPrefs.setOn(context, false)
        assertFalse(DictationPrefs.isOn(context))
        assertEquals(BubblePlace.Default, DictationPrefs.place(context))
        DictationPrefs.setPlace(context, BubblePlace(0f, 0.3f))
        assertEquals(BubblePlace(0f, 0.3f), DictationPrefs.place(context))
    }

    @After fun keyOff() = DictationPrefs.setOn(context, false)

    /** Accessibility on: the binding keeps the key, so no notification while it waits. */
    @Test fun keptByAccessibilityTheKeyShowsNoNotification() {
        shadowOf(context).grantPermissions(android.Manifest.permission.RECORD_AUDIO)
        DictationPrefs.setOn(context, true)
        val controller = Robolectric.buildService(DictationService::class.java).create().bind()
        assertNull(shadowOf(controller.get()).lastForegroundNotification)
    }

    /** Without accessibility it is a foreground service with its notification; accessibility coming on takes that away. */
    @Test fun theNotificationGoesWhenAccessibilityTakesTheKeyOver() {
        shadowOf(context).grantPermissions(android.Manifest.permission.RECORD_AUDIO)
        DictationPrefs.setOn(context, true)
        val controller = Robolectric.buildService(DictationService::class.java).create().startCommand(0, 1)
        val shadow = shadowOf(controller.get())
        val shown = shadow.lastForegroundNotification
        assertNotNull(shown)
        assertEquals("hide and off", 2, shown!!.actions.size)
        assertFalse(shadow.isForegroundStopped)
        controller.bind()
        assertTrue(shadow.isForegroundStopped)
    }

    /** The engine never shares a process with the recorder: a crash in it cannot stop a meeting's recording. */
    @Test fun dictationRunsInItsOwnProcess() {
        val pm = context.packageManager
        val key = pm.getServiceInfo(ComponentName(context, DictationService::class.java), PackageManager.GET_META_DATA)
        val insert = pm.getServiceInfo(ComponentName(context, InsertService::class.java), PackageManager.GET_META_DATA)
        assertEquals("${context.packageName}:dictation", key.processName)
        assertEquals(key.processName, insert.processName)
        assertEquals(android.Manifest.permission.BIND_ACCESSIBILITY_SERVICE, insert.permission)
    }
}
