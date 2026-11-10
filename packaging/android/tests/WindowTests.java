package app.cinnabar.windowtests;

import android.app.AlertDialog;
import android.app.Instrumentation;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Rect;
import java.io.File;
import java.io.FileOutputStream;
import android.os.Bundle;
import android.view.DisplayCutout;
import android.view.WindowInsets;
import java.util.Arrays;
import java.util.Collections;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.function.BooleanSupplier;

/** Device regressions for navigation visibility, focus recovery and independent keyboard control. */
public final class WindowTests extends Instrumentation {
    private static final int RESULT_OK = -1;
    private WindowActivity activity;

    @Override public void onCreate(Bundle arguments) {
        super.onCreate(arguments);
        start();
    }

    @Override public void onStart() {
        Bundle results = new Bundle();
        try {
            cutoutInsetsPreserveKeyboardOverlap();
            Intent launch = new Intent(getTargetContext(), WindowActivity.class)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            activity = (WindowActivity) startActivitySync(launch);
            await(() -> activity.hasWindowFocus(), "window focus");
            await(() -> visible(WindowInsets.Type.navigationBars())
                    && hasObstruction(WindowInsets.Type.navigationBars()), "baseline navigation bar geometry");
            capture("window-before.png");
            runOnMainSync(() -> {
                activity.immersive = new ImmersiveMode(activity.getWindow());
                activity.immersive.restore();
            });
            await(() -> systemBarsHidden(), "initial immersive mode");
            capture("window-after.png");

            runOnMainSync(() -> {
                activity.editor.requestFocus();
                activity.getWindow().getInsetsController().show(WindowInsets.Type.ime());
            });
            await(() -> visible(WindowInsets.Type.ime()) && hasObstruction(WindowInsets.Type.ime()), "keyboard opens in immersive mode");
            runOnMainSync(() -> activity.immersive.restore());
            await(() -> activity.animations.isEmpty(), "restoration animations finish");
            getUiAutomation().waitForIdle(1000, 5000);
            runOnMainSync(() -> {
                if (!visible(WindowInsets.Type.ime()) || !hasObstruction(WindowInsets.Type.ime())) {
                    throw new AssertionError("bar restoration preserves the keyboard after settling");
                }
            });
            capture("window-keyboard.png");
            runOnMainSync(() -> activity.getWindow().getInsetsController().hide(WindowInsets.Type.ime()));
            await(() -> !visible(WindowInsets.Type.ime())
                    && systemBarsHidden(), "keyboard dismissal restores immersive mode");

            AlertDialog[] dialog = new AlertDialog[1];
            runOnMainSync(() -> {
                dialog[0] = new AlertDialog.Builder(activity).setMessage("Window focus test").create();
                dialog[0].show();
            });
            await(() -> !activity.hasWindowFocus(), "dialog takes window focus");
            runOnMainSync(() -> dialog[0].dismiss());
            await(() -> activity.hasWindowFocus() && systemBarsHidden(),
                    "dialog dismissal restores immersive mode");
            results.putString("stream", "PASS: cutout geometry, initial hiding, keyboard independence, dismissal and focus recovery\n");
            runOnMainSync(() -> activity.finish());
            finish(RESULT_OK, results);
        } catch (Throwable failure) {
            results.putString("stream", "FAIL: " + failure + "\n");
            if (activity != null) runOnMainSync(() -> activity.finish());
            finish(0, results);
        }
    }

    /** Exercises the production geometry path without requiring a physically notched screen. */
    private void cutoutInsetsPreserveKeyboardOverlap() {
        WindowInsets left = new WindowInsets.Builder().setDisplayCutout(
                new DisplayCutout(new Rect(96, 24, 0, 12), Collections.emptyList())).build();
        WindowInsets right = new WindowInsets.Builder().setDisplayCutout(
                new DisplayCutout(new Rect(0, 24, 96, 12), Collections.emptyList())).build();
        checkInsets(left, 0, new int[] {96, 24, 0, 12});
        checkInsets(left, 360, new int[] {96, 24, 0, 360});
        checkInsets(right, 360, new int[] {0, 24, 96, 360});
        checkInsets(right, 0, new int[] {0, 24, 96, 12});
        checkInsets(new WindowInsets.Builder().build(), 280, new int[] {0, 0, 0, 280});
        checkInsets(new WindowInsets.Builder().build(), 0, new int[4]);
    }

    /** Checks that cutouts and bottom obstructions combine without double-counting overlap. */
    private void checkInsets(WindowInsets insets, int bottom, int[] expected) {
        int[] actual = WindowGeometry.safeAreaInsetsPixels(insets, bottom);
        if (!Arrays.equals(actual, expected)) {
            throw new AssertionError("safe area: " + Arrays.toString(actual)
                    + ", expected " + Arrays.toString(expected));
        }
    }

    /** Captures only this isolated window fixture after Android finishes its animations. */
    private void capture(String name) throws Exception {
        await(() -> activity.animations.isEmpty(), "capture animations finish");
        getUiAutomation().waitForIdle(1000, 5000);
        Bitmap image = getUiAutomation().takeScreenshot();
        if (image == null) throw new AssertionError("screen capture unavailable");
        try (FileOutputStream output = new FileOutputStream(new File(getTargetContext().getFilesDir(), name))) {
            if (!image.compress(Bitmap.CompressFormat.PNG, 100, output)) {
                throw new AssertionError("screen capture failed");
            }
        } finally {
            image.recycle();
        }
    }

    /** A hidden status bar alone must not make visible navigation pass this check. */
    private boolean systemBarsHidden() {
        return !visible(WindowInsets.Type.navigationBars())
                && !visible(WindowInsets.Type.statusBars())
                && !visible(WindowInsets.Type.captionBar());
    }

    /** Requires actual occupied screen area, including navigation on a landscape side edge. */
    private boolean hasObstruction(int type) {
        android.graphics.Insets insets = activity.getWindow().getDecorView().getRootWindowInsets().getInsets(type);
        return insets.left > 0 || insets.top > 0 || insets.right > 0 || insets.bottom > 0;
    }

    /** Reads current platform visibility on the UI thread. */
    private boolean visible(int type) {
        WindowInsets insets = activity.getWindow().getDecorView().getRootWindowInsets();
        return insets != null && insets.isVisible(type);
    }

    /** Waits for real layout events, with a bounded failure if Android never applies the change. */
    private void await(BooleanSupplier condition, String description) throws InterruptedException {
        CountDownLatch ready = new CountDownLatch(1);
        runOnMainSync(() -> {
            activity.insetsChanged = () -> { if (condition.getAsBoolean()) ready.countDown(); };
            activity.insetsChanged.run();
            activity.editor.requestApplyInsets();
        });
        if (!ready.await(30, TimeUnit.SECONDS)) throw new AssertionError(description);
        runOnMainSync(() -> activity.insetsChanged = () -> {});
    }
}
