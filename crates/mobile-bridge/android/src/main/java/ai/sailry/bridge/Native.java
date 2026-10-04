package ai.sailry.bridge;

import android.content.Context;

/** Call from application startup before Dart opens the Rust controller. */
public final class Native {
    static { System.loadLibrary("sailry_mobile_bridge"); }

    private Native() {}

    public static void initialize(Context context) {
        if (context == null) throw new IllegalArgumentException("application context is required");
        init(context.getApplicationContext());
    }

    private static native void init(Context context);
}
