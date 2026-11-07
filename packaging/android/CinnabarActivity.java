package @APPLICATION_ID@;

import android.app.NativeActivity;
import android.content.Intent;
import android.graphics.Rect;
import android.hardware.input.InputManager;
import android.os.Build;
import android.os.Bundle;
import android.view.InputDevice;
import android.view.MotionEvent;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowManager;
import java.util.HashSet;
import java.util.Set;

/** Hosts the client, whose JSON-UI engine draws every game screen. */
public final class CinnabarActivity extends NativeActivity {
    private volatile int bottomObstruction;
    private StartupScreen startup;
    private volatile boolean startupActive;
    private volatile long startupGeneration;
    private ImmersiveMode immersiveMode;
    private final Set<Integer> physicalMice = new HashSet<>();
    private final Set<Integer> physicalKeyboards = new HashSet<>();
    private int lastReportedMice = -1;
    private int lastReportedKeyboards = -1;
    private int mLastButtonState = 0;
    private InputManager.InputDeviceListener inputDeviceListener;

    @Override protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        FilePicker.attach(this);
        startup = new StartupScreen(this);
        addContentView(startup, new android.view.ViewGroup.LayoutParams(
                android.view.ViewGroup.LayoutParams.MATCH_PARENT, android.view.ViewGroup.LayoutParams.MATCH_PARENT));
        getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING);
        immersiveMode = new ImmersiveMode(getWindow());
        final View decor = getWindow().getDecorView();
        decor.setOnApplyWindowInsetsListener((view, insets) -> {
            updateKeyboardGeometry(decor, insets);
            return view.onApplyWindowInsets(insets);
        });
        decor.getViewTreeObserver().addOnGlobalLayoutListener(() ->
                updateKeyboardGeometry(decor, decor.getRootWindowInsets()));
        decor.requestApplyInsets();
        if (Build.VERSION.SDK_INT >= 26) {
            decor.setOnCapturedPointerListener((view, event) -> {
                int action = event.getActionMasked();
                if (action == MotionEvent.ACTION_MOVE) {
                    float dx = event.getAxisValue(MotionEvent.AXIS_RELATIVE_X);
                    float dy = event.getAxisValue(MotionEvent.AXIS_RELATIVE_Y);
                    if (dx != 0.0f || dy != 0.0f) {
                        try {
                            nativeOnMouseMotion(dx, dy);
                        } catch (UnsatisfiedLinkError e) {
                            // Ignored
                        }
                    }
                    return true;
                }
                if (action == MotionEvent.ACTION_SCROLL) {
                    float vscroll = event.getAxisValue(MotionEvent.AXIS_VSCROLL);
                    float hscroll = event.getAxisValue(MotionEvent.AXIS_HSCROLL);
                    try {
                        nativeOnMouseScroll(vscroll, hscroll);
                    } catch (UnsatisfiedLinkError e) {
                        // Ignored
                    }
                    return true;
                }
                int buttonState = event.getButtonState();
                dispatchMouseButtonDiff(buttonState);
                return true;
            });
        }
        final InputManager im = (InputManager) getSystemService(INPUT_SERVICE);
        if (im != null) {
            inputDeviceListener = new InputManager.InputDeviceListener() {
                @Override public void onInputDeviceAdded(int deviceId) { handleDeviceAdded(deviceId); }
                @Override public void onInputDeviceRemoved(int deviceId) { handleDeviceRemoved(deviceId); }
                @Override public void onInputDeviceChanged(int deviceId) { handleDeviceChanged(deviceId); }
            };
            im.registerInputDeviceListener(inputDeviceListener, null);
            initConnectedDevices(im);
        }
    }

    @Override protected void onResume() {
        startupGeneration++;
        startupActive = true;
        super.onResume();
        immersiveMode.restore();
        notifyDeviceStateChanged();
    }

    @Override protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        if (!FilePicker.onResult(requestCode, resultCode, data)) {
            super.onActivityResult(requestCode, resultCode, data);
        }
    }

    @Override protected void onPause() {
        startupActive = false;
        startupGeneration++;
        clearHeldMouseButtons();
        super.onPause();
    }

    /** Native startup reports work on its own thread; only the activity thread owns views. */
    public void startupProgress(String message, long generation) {
        final boolean active = startupActive;
        runOnUiThread(() -> {
            if (active && startupActive && generation == startupGeneration
                    && !isFinishing() && !isDestroyed() && startup != null) startup.update(message);
        });
    }

    /** Lets native rendering invalidate readiness even when a pause preserves its surface. */
    public long startupGeneration() { return startupGeneration; }

    /** Keeps a native startup failure readable until the player closes the activity. */
    public void startupFailed(String message) {
        runOnUiThread(() -> {
            if (isFinishing() || isDestroyed()) return;
            startupActive = false;
            new android.app.AlertDialog.Builder(this)
                    .setTitle("@PRODUCT_NAME@ failed")
                    .setMessage(message)
                    .setPositiveButton("Close", (dialog, which) -> finish())
                    .setOnCancelListener(dialog -> finish())
                    .show();
        });
    }

    /** Publishes the keyboard overlap while leaving the Vulkan surface at full size. */
    private void updateKeyboardGeometry(View decor, WindowInsets insets) {
        if (insets == null) return;
        if (Build.VERSION.SDK_INT >= 30) {
            bottomObstruction = insets.getInsets(
                    WindowInsets.Type.ime() | WindowInsets.Type.systemBars()).bottom;
            immersiveMode.keyboardVisibilityChanged(insets.isVisible(WindowInsets.Type.ime()));
        } else {
            Rect visible = new Rect();
            int[] location = new int[2];
            decor.getWindowVisibleDisplayFrame(visible);
            decor.getLocationOnScreen(location);
            bottomObstruction = Math.max(0, location[1] + decor.getHeight() - visible.bottom);
            immersiveMode.keyboardVisibilityChanged(bottomObstruction > insets.getStableInsetBottom());
        }
    }

    /** Returns bottom system-window obstruction relative to the full native surface. */
    public int bottomObstructionPixels() {
        return bottomObstruction;
    }

    @Override protected void onDestroy() {
        if (inputDeviceListener != null) {
            final InputManager im = (InputManager) getSystemService(INPUT_SERVICE);
            if (im != null) im.unregisterInputDeviceListener(inputDeviceListener);
        }
        // The native loop never returns on destroy, which NativeActivity's teardown waits for,
        // and Bevy cannot start twice in one process anyway.
        android.os.Process.killProcess(android.os.Process.myPid());
        super.onDestroy();
    }

    /** Distinguishes physical mice from touchscreens, virtual devices, and trackpads. */
    static boolean isPhysicalMouse(InputDevice dev) {
        if (dev == null || dev.isVirtual() || dev.getId() <= 0) return false;
        final int sources = dev.getSources();
        return (sources & InputDevice.SOURCE_MOUSE) == InputDevice.SOURCE_MOUSE
                || (sources & InputDevice.SOURCE_MOUSE_RELATIVE) != 0;
    }

    /** Distinguishes real physical alphanumeric keyboards from virtual IMEs and hardware buttons. */
    static boolean isPhysicalKeyboard(InputDevice dev) {
        if (dev == null || dev.isVirtual() || dev.getId() <= 0) return false;
        final int sources = dev.getSources();
        final boolean hasKeyboard = (sources & InputDevice.SOURCE_KEYBOARD) == InputDevice.SOURCE_KEYBOARD;
        final boolean isAlphabetic = dev.getKeyboardType() == InputDevice.KEYBOARD_TYPE_ALPHABETIC;
        return hasKeyboard && isAlphabetic;
    }

    private void handleDeviceAdded(int deviceId) {
        final InputManager im = (InputManager) getSystemService(INPUT_SERVICE);
        if (im == null) return;
        final InputDevice dev = im.getInputDevice(deviceId);
        if (dev == null) return;
        boolean changed = false;
        if (isPhysicalMouse(dev)) {
            changed |= physicalMice.add(deviceId);
        }
        if (isPhysicalKeyboard(dev)) {
            changed |= physicalKeyboards.add(deviceId);
        }
        if (changed) notifyDeviceStateChanged();
    }

    private void handleDeviceRemoved(int deviceId) {
        final boolean changed = physicalMice.remove(deviceId) | physicalKeyboards.remove(deviceId);
        if (changed) notifyDeviceStateChanged();
    }

    private void handleDeviceChanged(int deviceId) {
        physicalMice.remove(deviceId);
        physicalKeyboards.remove(deviceId);
        handleDeviceAdded(deviceId);
    }

    private void initConnectedDevices(InputManager im) {
        final int[] ids = im.getInputDeviceIds();
        for (int id : ids) {
            final InputDevice dev = im.getInputDevice(id);
            if (dev != null) {
                if (isPhysicalMouse(dev)) physicalMice.add(id);
                if (isPhysicalKeyboard(dev)) physicalKeyboards.add(id);
            }
        }
        notifyDeviceStateChanged();
    }

    private void notifyDeviceStateChanged() {
        final int mice = physicalMice.size();
        final int keyboards = physicalKeyboards.size();
        if (mice != lastReportedMice || keyboards != lastReportedKeyboards) {
            lastReportedMice = mice;
            lastReportedKeyboards = keyboards;
            try {
                nativeOnDeviceStateChange(mice, keyboards);
            } catch (UnsatisfiedLinkError e) {
                // In case activity starts before native library methods are bound
            }
        }
    }

    @Override public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (!hasFocus) {
            clearHeldMouseButtons();
        }
    }

    @Override public void onPointerCaptureChange(boolean hasCapture) {
        super.onPointerCaptureChange(hasCapture);
        try {
            nativeOnPointerCaptureChange(hasCapture);
        } catch (UnsatisfiedLinkError e) {
            // Ignored if called during early teardown
        }
        if (!hasCapture) {
            clearHeldMouseButtons();
        }
    }

    @Override public boolean dispatchTouchEvent(MotionEvent ev) {
        if ((ev.getSource() & InputDevice.SOURCE_MOUSE) != 0) {
            handleMouseMotionEvent(ev);
            return true;
        }
        return super.dispatchTouchEvent(ev);
    }

    @Override public boolean onGenericMotionEvent(MotionEvent event) {
        if ((event.getSource() & InputDevice.SOURCE_MOUSE) != 0) {
            handleMouseMotionEvent(event);
            return true;
        }
        return super.onGenericMotionEvent(event);
    }

    private void handleMouseMotionEvent(MotionEvent event) {
        int action = event.getActionMasked();
        if (action == MotionEvent.ACTION_SCROLL) {
            float vscroll = event.getAxisValue(MotionEvent.AXIS_VSCROLL);
            float hscroll = event.getAxisValue(MotionEvent.AXIS_HSCROLL);
            try {
                nativeOnMouseScroll(vscroll, hscroll);
            } catch (UnsatisfiedLinkError e) {
                // Ignored if called before native binding
            }
            return;
        }
        if (action == MotionEvent.ACTION_HOVER_MOVE) {
            return;
        }
        int buttonState = event.getButtonState();
        dispatchMouseButtonDiff(buttonState);
    }

    public void setPointerCaptureEnabled(boolean enabled) {
        runOnUiThread(() -> {
            if (Build.VERSION.SDK_INT >= 26) {
                View decor = getWindow().getDecorView();
                if (decor == null) return;
                if (enabled) {
                    decor.requestPointerCapture();
                } else {
                    decor.releasePointerCapture();
                }
            }
        });
    }

    private void dispatchMouseButtonDiff(int currentButtonState) {
        int changed = currentButtonState ^ mLastButtonState;
        if (changed == 0) return;

        for (int i = 0; i < 5; i++) {
            int mask = 1 << i;
            if ((changed & mask) != 0) {
                boolean pressed = (currentButtonState & mask) != 0;
                try {
                    nativeOnMouseButton(mask, pressed);
                } catch (UnsatisfiedLinkError e) {
                    // Ignored if called before native binding
                }
            }
        }
        mLastButtonState = currentButtonState;
    }

    public void clearHeldMouseButtons() {
        if (mLastButtonState != 0) {
            dispatchMouseButtonDiff(0);
        }
    }

    private static native void nativeOnDeviceStateChange(int physicalMice, int physicalKeyboards);
    private static native void nativeOnMouseMotion(float dx, float dy);
    private static native void nativeOnMouseButton(int buttonMask, boolean pressed);
    private static native void nativeOnMouseScroll(float vscroll, float hscroll);
    private static native void nativeOnPointerCaptureChange(boolean hasCapture);
}
