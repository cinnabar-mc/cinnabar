package @APPLICATION_ID@;

import android.os.Build;
import android.view.DisplayCutout;
import android.view.WindowInsets;

/** Keeps native UI inside display cutouts and the keyboard without shrinking the world. */
final class WindowGeometry {
    /** Combines cutout and bottom-window obstructions in left, top, right, bottom order. */
    static int[] safeAreaInsetsPixels(WindowInsets insets, int bottomObstruction) {
        DisplayCutout cutout = Build.VERSION.SDK_INT >= 28 ? insets.getDisplayCutout() : null;
        if (cutout == null) return new int[] {0, 0, 0, bottomObstruction};
        return new int[] {
                cutout.getSafeInsetLeft(), cutout.getSafeInsetTop(), cutout.getSafeInsetRight(),
                Math.max(cutout.getSafeInsetBottom(), bottomObstruction)
        };
    }
}
