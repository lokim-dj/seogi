package io.github.lokimdj.seogi.data

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** The few things the user chooses, in the app's private preferences. */
class Settings(context: Context) {
    private val prefs = context.getSharedPreferences("seogi", Context.MODE_PRIVATE)

    private val _email = MutableStateFlow(prefs.getString(KEY_EMAIL, null))
    private val _wifiOnly = MutableStateFlow(prefs.getBoolean(KEY_WIFI_ONLY, false))

    /**
     * The Google account the recordings go to: null before the first login,
     * empty when signed in but the address could not be read.
     */
    val email: StateFlow<String?> = _email.asStateFlow()

    /** Upload only on an unmetered network. Off by default. */
    val wifiOnly: StateFlow<Boolean> = _wifiOnly.asStateFlow()

    var onboarded: Boolean
        get() = prefs.getBoolean(KEY_ONBOARDED, false)
        set(v) = prefs.edit().putBoolean(KEY_ONBOARDED, v).apply()

    /** The id of the app-made `서기 폰 녹음` folder in the signed-in Drive. */
    var driveFolderId: String?
        get() = prefs.getString(KEY_FOLDER, null)
        set(v) = prefs.edit().putString(KEY_FOLDER, v).apply()

    fun setEmail(value: String?) {
        if (value != _email.value) driveFolderId = null
        prefs.edit().putString(KEY_EMAIL, value).apply()
        _email.value = value
    }

    fun setWifiOnly(value: Boolean) {
        prefs.edit().putBoolean(KEY_WIFI_ONLY, value).apply()
        _wifiOnly.value = value
    }

    private companion object {
        const val KEY_EMAIL = "google_email"
        const val KEY_WIFI_ONLY = "wifi_only"
        const val KEY_ONBOARDED = "onboarded"
        const val KEY_FOLDER = "drive_folder_id"
    }
}
