pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}
rootProject.name = "seogi-android"
include(":app")
// Throwaway test app for the phone's speech engine (android/spike/README.md).
// Only with -Pseogi.spike, so CI and normal builds never need its downloads.
if (providers.gradleProperty("seogi.spike").isPresent) include(":spike")
