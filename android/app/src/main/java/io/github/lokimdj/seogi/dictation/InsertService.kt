package io.github.lokimdj.seogi.dictation

import android.accessibilityservice.AccessibilityService
import android.content.ClipData
import android.content.ClipboardManager
import android.content.res.Configuration
import android.os.Bundle
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

    override fun onServiceConnected() {
        instance = this
        check()
        listener?.invoke()
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        when (event?.eventType) {
            AccessibilityEvent.TYPE_VIEW_FOCUSED,
            AccessibilityEvent.TYPE_WINDOWS_CHANGED,
            AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED -> check()
        }
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
    }

    /** Null when Android does not give the window list. */
    private fun keyboardShowing(): Boolean? {
        val list = try { windows } catch (e: Exception) { null }
        if (list.isNullOrEmpty()) return null
        return list.any { it.type == AccessibilityWindowInfo.TYPE_INPUT_METHOD }
    }

    private fun editableFocused(): Boolean =
        try { findFocus(AccessibilityNodeInfo.FOCUS_INPUT)?.isEditable == true } catch (e: Exception) { false }

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
        val node = try { findFocus(AccessibilityNodeInfo.FOCUS_INPUT) } catch (e: Exception) { null } ?: return false
        return Inserter.insert(NodeField(node), text)
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
