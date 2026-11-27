package io.github.lokimdj.seogi.dictation

import android.accessibilityservice.AccessibilityService
import android.content.ClipData
import android.content.ClipboardManager
import android.os.Bundle
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo

/**
 * The accessibility part of dictation, switched on by the user in Android's
 * settings: it finds the text field that has the cursor in the app in front
 * and puts the spoken text there. It reads nothing else, keeps nothing and
 * sends nothing; it acts only when the floating key has text to put in.
 * Runs in the `:dictation` process beside [DictationService].
 */
class InsertService : AccessibilityService() {

    override fun onServiceConnected() {
        instance = this
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {}

    override fun onInterrupt() {}

    override fun onDestroy() {
        if (instance === this) instance = null
        super.onDestroy()
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
    }
}
