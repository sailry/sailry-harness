package ai.sailry.bridge;

import android.content.Context;

public final class Native {
    static {
        System.loadLibrary("sailry_iroh");
        System.loadLibrary("sailry_mobile");
    }

    private Native() {}

    public static void initialize(Context context) {
        if (context == null) throw new IllegalArgumentException("application context is required");
        init(context.getApplicationContext());
    }

    private static native void init(Context context);
}
