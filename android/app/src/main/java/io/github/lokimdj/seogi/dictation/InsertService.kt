package io.github.lokimdj.seogi.dictation

import android.accessibilityservice.AccessibilityService
import android.content.ClipData
import android.content.ClipboardManager
import android.content.res.Configuration
import android.os.Bundle
import android.os.SystemClock
import android.util.Log
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo

/**
 * The accessibility part of dictation, switched on by the user in Android's
 * settings: it finds the text field that has the cursor in the app in front
 * and puts the spoken text there. It also notices when a field takes the
 * cursor or the keyboard comes up, so the floating key shows only while the
 * user types ([Typing]); for that it looks at the kind of window and whether
 * the focused field takes text, never at what is written or shown. It keeps
 * nothing and sends nothing. Runs in the `:dictation` process beside
 * [DictationService].
 */
class InsertService : AccessibilityService() {
    /**
     * The last field that took text while the user typed. Touching the key
     * or the keyboard closing can take the focus from it before the text is
     * ready; it is used then ([InsertTarget]). Only the reference is kept.
     */
    private var last: AccessibilityNodeInfo? = null
    private var lastResume = 0L

    override fun onServiceConnected() {
        instance = this
        check()
        listener?.invoke()
        // Bound again after an update or a restart: the key the user left on comes back.
        DictationService.resume(this, "accessibility bound")
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        when (event?.eventType) {
            AccessibilityEvent.TYPE_VIEW_FOCUSED -> {
                remember(event.source)
                check()
            }
            AccessibilityEvent.TYPE_VIEW_TEXT_SELECTION_CHANGED -> remember(event.source)
            AccessibilityEvent.TYPE_WINDOWS_CHANGED,
            AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED -> check()
        }
    }

    private fun remember(node: AccessibilityNodeInfo?) {
        if (node != null && node.isEditable) last = node
    }

    /** Looks again whether the user is typing, and tells the key when that changed. */
    private fun check() {
        val now = try {
            Typing.from(keyboardShowing(), editableFocused(), hardKeyboard())
        } catch (e: Exception) {
            false
        }
        if (now != typing) {
            typing = now
            listener?.invoke()
        }
        // The user types but the key's service is not running (Android stopped
        // it, or it never came back): try again, at most twice a minute.
        if (now && !DictationService.running && SystemClock.elapsedRealtime() - lastResume > RESUME_EVERY_MS) {
            lastResume = SystemClock.elapsedRealtime()
            DictationService.resume(this, "typing")
        }
    }

    /** Null when Android does not give the window list. */
    private fun keyboardShowing(): Boolean? {
        val list = try { windows } catch (e: Exception) { null }
        if (list.isNullOrEmpty()) return null
        return list.any { it.type == AccessibilityWindowInfo.TYPE_INPUT_METHOD }
    }

    private fun editableFocused(): Boolean {
        val node = focused() ?: return false
        remember(node)
        return node.isEditable
    }

    /**
     * The field with the input focus: Android's answer first, then each
     * window's own (with two apps side by side on a Fold, the answer can
     * come from a window without a text field).
     */
    private fun focused(): AccessibilityNodeInfo? {
        val first = try { findFocus(AccessibilityNodeInfo.FOCUS_INPUT) } catch (e: Exception) { null }
        if (first?.isEditable == true) return first
        val list = try { windows } catch (e: Exception) { null }.orEmpty()
        val inWindow = list.asSequence()
            .filter { it.type == AccessibilityWindowInfo.TYPE_APPLICATION }
            .mapNotNull { w -> try { w.root?.findFocus(AccessibilityNodeInfo.FOCUS_INPUT) } catch (e: Exception) { null } }
            .firstOrNull { it.isEditable }
        return inWindow ?: first
    }

    private fun hardKeyboard(): Boolean {
        val c = resources.configuration
        return c.keyboard != Configuration.KEYBOARD_NOKEYS && c.hardKeyboardHidden == Configuration.HARDKEYBOARDHIDDEN_NO
    }

    override fun onInterrupt() {}

    override fun onUnbind(intent: android.content.Intent?): Boolean {
        gone()
        return super.onUnbind(intent)
    }

    override fun onDestroy() {
        gone()
        super.onDestroy()
    }

    /** Switched off: the key can no longer tell when the user types. */
    private fun gone() {
        last = null
        if (instance !== this) return
        instance = null
        typing = false
        listener?.invoke()
    }

    /**
     * Puts [text] at the cursor of the focused field. False when no field
     * has the cursor or the app would not take it; the caller then copies.
     */
    fun insert(text: String): Boolean {
        val shown = try { windows.map { it.id }.toSet() } catch (e: Exception) { emptySet() }
        val choice = InsertTarget.pick(
            focused(), last,
            editable = { it.isEditable },
            onScreen = { n -> n.refresh() && (shown.isEmpty() || n.windowId in shown) },
        )
        val node = choice.field
        if (node == null) {
            Log.i(TAG, "copying instead of typing: ${choice.why}")
            return false
        }
        if (choice.refocus) node.performAction(AccessibilityNodeInfo.ACTION_FOCUS)
        val typed = Inserter.insert(NodeField(node), text)
        Log.i(TAG, if (typed) "typed: ${choice.why}" else "the field took neither new text nor a paste (${choice.why}); copying")
        return typed
    }

    /** The focused field as [Inserter] sees it. */
    private inner class NodeField(private val node: AccessibilityNodeInfo) : TextField {
        override val editable get() = node.isEditable
        override val password get() = node.isPassword
        override val text get() = if (node.isShowingHintText) "" else node.text?.toString().orEmpty()
        override val selectionStart get() = node.textSelectionStart
        override val selectionEnd get() = node.textSelectionEnd

        override fun setText(text: String) = node.performAction(
            AccessibilityNodeInfo.ACTION_SET_TEXT,
            Bundle().apply { putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text) },
        )

        override fun setCursor(at: Int) {
            node.performAction(
                AccessibilityNodeInfo.ACTION_SET_SELECTION,
                Bundle().apply {
                    putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_START_INT, at)
                    putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_END_INT, at)
                },
            )
        }

        override fun paste(text: String): Boolean {
            getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText("서기", text))
            return node.performAction(AccessibilityNodeInfo.ACTION_PASTE)
        }
    }

    companion object {
        private const val TAG = "SeogiDictation"
        private const val RESUME_EVERY_MS = 30_000L

        /** The running service, when the user has switched it on. Read and set on the main thread. */
        @Volatile
        var instance: InsertService? = null
            private set

        /** Whether the user is typing now, as last seen. Main thread. */
        var typing = false
            private set

        /** Told (on the main thread) when [instance] or [typing] changes; set by [DictationService]. */
        var listener: (() -> Unit)? = null
    }
}
