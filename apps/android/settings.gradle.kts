// Both lists try Google's mirror of Maven Central before Maven Central itself, which rate-limits
// large downloads (429) when a build starts from an empty cache. Every file is still checked
// against gradle/verification-metadata.xml, so the mirror can't change what we build with.
// (pluginManagement is compiled on its own, so the address is written out twice.)
pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        maven("https://maven-central.storage-download.googleapis.com/maven2/")
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        maven("https://maven-central.storage-download.googleapis.com/maven2/")
        mavenCentral()
    }
}

rootProject.name = "shyspace"
include(":app")
