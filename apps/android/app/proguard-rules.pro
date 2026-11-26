# UniFFI's generated bindings call the Rust core through JNA, which finds classes, fields
# and methods by name at run time.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uniffi.** { *; }
-dontwarn java.awt.**
