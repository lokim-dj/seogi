package io.github.lokimdj.seogi.upload

import android.accounts.Account
import android.app.Activity
import android.content.Context
import android.content.Intent
import androidx.credentials.CredentialManager
import androidx.credentials.GetCredentialRequest
import com.google.android.gms.auth.api.identity.AuthorizationRequest
import com.google.android.gms.auth.api.identity.AuthorizationResult
import com.google.android.gms.auth.api.identity.Identity
import com.google.android.gms.common.api.Scope
import com.google.android.libraries.identity.googleid.GetSignInWithGoogleOption
import com.google.android.libraries.identity.googleid.GoogleIdTokenCredential
import io.github.lokimdj.seogi.BuildConfig
import kotlinx.coroutines.tasks.await

/**
 * Google sign-in for Drive with Google Identity Services. The app asks for
 * one scope, `drive.file` (only files the app made). The Android OAuth client
 * is matched by package name and signing certificate, so no client secret or
 * ID is in the app. With an optional Web client ID in the build
 * (`seogi.googleWebClientId`), Credential Manager shows the account picker
 * first.
 */
object GoogleAuth {
    private fun request(email: String?): AuthorizationRequest =
        AuthorizationRequest.builder()
            .setRequestedScopes(listOf(Scope(DriveApi.SCOPE)))
            .apply { if (email != null) setAccount(Account(email, "com.google")) }
            .build()

    /** Account picker via Credential Manager; null when not configured or cancelled. */
    suspend fun pickAccount(activity: Activity): String? {
        val webId = BuildConfig.GOOGLE_WEB_CLIENT_ID
        if (webId.isBlank()) return null
        return try {
            val option = GetSignInWithGoogleOption.Builder(webId).build()
            val result = CredentialManager.create(activity)
                .getCredential(activity, GetCredentialRequest.Builder().addCredentialOption(option).build())
            GoogleIdTokenCredential.createFrom(result.credential.data).id
        } catch (e: Exception) {
            null
        }
    }

    /** Asks for Drive; the result either carries a token or needs the consent screen. */
    suspend fun authorize(context: Context, email: String?): AuthorizationResult =
        Identity.getAuthorizationClient(context).authorize(request(email)).await()

    fun resultFromIntent(context: Context, data: Intent?): AuthorizationResult =
        Identity.getAuthorizationClient(context).getAuthorizationResultFromIntent(data)

    /** A token without any screen, for background uploads. */
    suspend fun token(context: Context, email: String?): TokenResult = try {
        val r = authorize(context, email)
        val t = r.accessToken
        if (r.hasResolution() || t == null) TokenResult.NeedLogin else TokenResult.Ok(t)
    } catch (e: com.google.android.gms.common.api.ApiException) {
        TokenResult.NeedLogin
    }
}
