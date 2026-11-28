package io.github.lokimdj.seogi.dictation

import android.accessibilityservice.AccessibilityService
import android.app.KeyguardManager
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ComponentName
import android.content.Intent
import android.content.ServiceConnection
import android.content.res.Configuration
import android.graphics.Rect
import android.os.Bundle
import android.os.IBinder
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
 * user types ([Typing]), and that a field was changed by hand, so the key
 * steps back while the user types; for that it looks at the kind of window,
 * whether the focused field takes text and that a field changed, never at
 * what is written or shown. It keeps
 * nothing and sends nothing. Runs in the `:dictation` process beside
 * [DictationService], which it holds bound: Android keeps an accessibility
 * service running, so the key needs no notification of its own while it waits.
 */
class InsertService : AccessibilityService() {
    /**
     * The last field that took text while the user typed. Touching the key
     * or the keyboard closing can take the focus from it before the text is
     * ready; it is used then ([InsertTarget]). Only the reference is kept.
     */
    private var last: AccessibilityNodeInfo? = null
    private var wroteAt = 0L
    private var holdsKey = false
    private val key = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {}
        override fun onServiceDisconnected(name: ComponentName?) {}
    }

    override fun onServiceConnected() {
        instance = this
        // Also bound again after an update or a restart: the key the user left on comes back.
        holdKey()
        check()
        listener?.invoke()
    }

    /** Keeps [DictationService] alive for as long as this service is on; it shows the key when the user left it on. */
    private fun holdKey() {
        if (holdsKey) return
        holdsKey = try {
            bindService(Intent(this, DictationService::class.java), key, BIND_AUTO_CREATE)
        } catch (e: Exception) {
            Log.w(TAG, "could not hold the key's service", e)
            false
        }
    }

    private fun letKeyGo() {
        if (!holdsKey) return
        holdsKey = false
        try { unbindService(key) } catch (_: Exception) {}
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        when (event?.eventType) {
            AccessibilityEvent.TYPE_VIEW_FOCUSED -> {
                remember(event.source)
                check()
            }
            AccessibilityEvent.TYPE_VIEW_TEXT_SELECTION_CHANGED -> remember(event.source)
            // Only that a field changed is used, never what it holds; Seogi's own writing is not typing.
            AccessibilityEvent.TYPE_VIEW_TEXT_CHANGED ->
                if (OwnWrite.isTyping(SystemClock.uptimeMillis(), wroteAt)) keystroke?.invoke()
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
            Typing.from(keyboardShowing(), editableFocused(), hardKeyboard(), locked())
        } catch (e: Exception) {
            false
        }
        val top = try { keyboardEdge() } catch (e: Exception) { null }
        if (now != typing || top != keyboardTop) {
            typing = now
            keyboardTop = top
            listener?.invoke()
        }
    }

    private fun locked() = getSystemService(KeyguardManager::class.java)?.isKeyguardLocked == true

    /** The top edge of the on-screen keyboard on the screen, or null when none is up. */
    private fun keyboardEdge(): Int? {
        val ime = windows.firstOrNull { it.type == AccessibilityWindowInfo.TYPE_INPUT_METHOD } ?: return null
        val r = Rect()
        ime.getBoundsInScreen(r)
        return if (r.isEmpty) null else r.top
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

    /**
     * The field inside [node] when the focus is on a frame that takes no text
     * itself ([InnerField]). Only when writing ([InsertTarget] asks last):
     * walking the parts on every focus change would cost the phone for nothing.
     */
    private fun inner(node: AccessibilityNodeInfo): AccessibilityNodeInfo? {
        return try {
            InnerField.pick(
                node,
                children = { n -> (0 until n.childCount).mapNotNull { i -> n.getChild(i) } },
                // A field kept out of sight (some pages hold one for their own use) is not the user's.
                editable = { it.isEditable && it.isVisibleToUser },
                focused = { it.isFocused },
                hasCursor = { it.textSelectionStart >= 0 },
            )
        } catch (e: Exception) { null }
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
        letKeyGo()
        if (instance !== this) return
        instance = null
        typing = false
        keyboardTop = null
        listener?.invoke()
    }

    /**
     * Puts [text] at the cursor of the focused field. False when no field
     * has the cursor or the app would not take it; the caller then copies.
     */
    fun insert(text: String): Boolean {
        wroteAt = SystemClock.uptimeMillis()
        val shown = try { windows.map { it.id }.toSet() } catch (e: Exception) { emptySet() }
        val choice = InsertTarget.pick(
            focused(), last,
            editable = { it.isEditable },
            onScreen = { n -> n.refresh() && n.isVisibleToUser && (shown.isEmpty() || n.windowId in shown) },
            inside = ::inner,
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

        /** The running service, when the user has switched it on. Read and set on the main thread. */
        @Volatile
        var instance: InsertService? = null
            private set

        /** Whether the user is typing now, as last seen. Main thread. */
        var typing = false
            private set

        /** The top edge of the on-screen keyboard, as last seen; null when none is up. Main thread. */
        var keyboardTop: Int? = null
            private set

        /** Told (on the main thread) when [instance], [typing] or [keyboardTop] changes; set by [DictationService]. */
        var listener: (() -> Unit)? = null

        /** Told (on the main thread) when the user changed a field by hand; set by [DictationService]. */
        var keystroke: (() -> Unit)? = null
    }
}
