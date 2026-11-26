import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
}

// Per-user values that never enter the repository: android/local.properties
// (gitignored) or environment variables. See android/README.md.
val local = Properties().apply {
    val f = rootProject.file("local.properties")
    if (f.exists()) f.inputStream().use { load(it) }
}
fun localValue(key: String, env: String): String? =
    local.getProperty(key)?.takeIf { it.isNotBlank() } ?: System.getenv(env)?.takeIf { it.isNotBlank() }

android {
    namespace = "io.github.lokimdj.seogi"
    compileSdk = 35
    buildToolsVersion = "35.0.0"

    defaultConfig {
        applicationId = "io.github.lokimdj.seogi"
        minSdk = 33
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        // Optional: a Web OAuth client ID lets the app show Google's account
        // picker (Credential Manager) before asking for Drive. Drive itself
        // needs only the Android OAuth client (package name + SHA-1).
        val webClientId = localValue("seogi.googleWebClientId", "SEOGI_GOOGLE_WEB_CLIENT_ID") ?: ""
        buildConfigField("String", "GOOGLE_WEB_CLIENT_ID", "\"$webClientId\"")
    }

    val storeFile = localValue("seogi.signing.storeFile", "SEOGI_SIGNING_STORE_FILE")
    signingConfigs {
        if (storeFile != null) {
            create("upload") {
                this.storeFile = file(storeFile)
                storePassword = localValue("seogi.signing.storePassword", "SEOGI_SIGNING_STORE_PASSWORD")
                    ?: localValue("seogi.signing.passwordFile", "SEOGI_SIGNING_PASSWORD_FILE")?.let { file(it).readText().trim() }
                keyAlias = localValue("seogi.signing.keyAlias", "SEOGI_SIGNING_KEY_ALIAS") ?: "upload"
                keyPassword = storePassword
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            if (storeFile != null) signingConfig = signingConfigs.getByName("upload")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures {
        compose = true
        buildConfig = true
    }
    testOptions { unitTests.isReturnDefaultValues = true }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.service)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.foundation)
    implementation(libs.androidx.compose.ui.tooling.preview)
    debugImplementation(libs.androidx.compose.ui.tooling)
    implementation(libs.androidx.work.runtime)
    implementation(libs.androidx.credentials)
    implementation(libs.androidx.credentials.play)
    implementation(libs.googleid)
    implementation(libs.play.services.auth)
    implementation(libs.kotlinx.serialization.json)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.coroutines.play.services)
    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
}
