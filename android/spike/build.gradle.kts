// Throwaway test app (spike): which on-phone Korean speech engine is fast and
// right enough for dictation. Built only with -Pseogi.spike; see README.md.
plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "io.github.lokimdj.seogi.spike"
    compileSdk = 35
    buildToolsVersion = "35.0.0"
    ndkVersion = "29.0.14206865"

    defaultConfig {
        // Its own package, so it never touches the installed Seogi or its recordings.
        applicationId = "io.github.lokimdj.seogi.spike"
        minSdk = 33
        targetSdk = 35
        versionCode = 1
        versionName = "0.0.1-spike"
        ndk { abiFilters += "arm64-v8a" }
        val tarball = System.getenv("SEOGI_TRANSCRIBE_TARBALL")
            ?: "${System.getProperty("user.home")}/seogi/spike-cache/transcribe.cpp-v0.4.2.tar.gz"
        externalNativeBuild {
            cmake { arguments += listOf("-DANDROID_STL=c++_shared", "-DCMAKE_BUILD_TYPE=Release", "-DSEOGI_TRANSCRIBE_TARBALL=$tarball") }
        }
    }
    buildTypes {
        // The native engine is slow without optimisation; debug builds still compile it optimised.
        debug { externalNativeBuild { cmake { arguments += "-DCMAKE_BUILD_TYPE=Release" } } }
    }
    externalNativeBuild {
        cmake {
            path = file("src/main/cpp/CMakeLists.txt")
            version = "3.31.6"
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }
    packaging { jniLibs { useLegacyPackaging = false } }
}

dependencies {
    // sherpa-onnx (Apache 2.0), downloaded by tools/fetch.sh, not kept in git.
    implementation(files("libs/sherpa-onnx-2.0.3.aar"))
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.foundation)
    implementation("androidx.compose.material3:material3")
    testImplementation(libs.junit)
}
