# The UniFFI/JNA bridge is called through its generated Kotlin classes; keep
# the JNA callback interface signatures intact for R8 builds.
-keep class dev.eightbitdo.mobile.core.** { *; }
-keep class com.sun.jna.** { *; }
-dontwarn java.awt.**
