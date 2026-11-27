# UniFFI's generated bindings call the Rust core through JNA, which finds classes, fields
# and methods by name at run time.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uniffi.** { *; }
-dontwarn java.awt.**
# rustls-platform-verifier's Kotlin half is called only from Rust, over JNI.
-keep, includedescriptorclasses class org.rustls.platformverifier.** { *; }
