package @APPLICATION_ID@;

import android.app.Activity;
import android.content.Intent;
import android.content.res.AssetManager;
import android.os.Bundle;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.security.GeneralSecurityException;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicLong;

/** Unpacks the prep kit and prepares game assets on the startup screen, then starts the game. */
public final class BootstrapActivity extends Activity {
    static { System.loadLibrary("@CLIENT_LIBRARY@"); }

    private static final String RESOURCES = "resources";

    /** Numbers setup attempts across activity instances, so a cancellation names its own. */
    private static final AtomicLong ATTEMPTS = new AtomicLong();
    /** One attempt at a time unpacks and prepares, across activity instances. */
    private static final Object SETUP = new Object();

    /** Null when the game can start, empty when the user quit, otherwise the failure. */
    private native String prepare(String filesDir, String nativeLibraryDir, long attempt);
    private native void cancel(long attempt);

    private volatile long attempt;
    private SetupScreen screen;
    private volatile CountDownLatch consentAnswer;

    /** Stays out of immersive mode, whose first-use system notice would cover the setup prompts. */
    @Override protected void onCreate(Bundle state) {
        super.onCreate(state);
        screen = new SetupScreen(this, this::finish);
        setContentView(screen);
        if (!StartupDiagnostics.showPreviousFailure(this, this::start)) start();
    }

    /** Back abandons setup; Android 12+ would otherwise only move a root activity to the back. */
    @Override public void onBackPressed() {
        finish();
    }

    private void start() {
        screen.restart();
        long current = ATTEMPTS.incrementAndGet();
        attempt = current;
        new Thread(() -> setup(current), "setup").start();
    }

    private void setup(long current) {
        String result;
        synchronized (SETUP) {
            try {
                if (isFinishing() || isDestroyed()) {
                    result = "";
                } else {
                    unpackResources();
                    result = prepare(getFilesDir().getAbsolutePath(),
                            getApplicationInfo().nativeLibraryDir, current);
                }
            } catch (Exception error) {
                result = error.toString();
            }
        }
        String outcome = result;
        runOnUiThread(() -> finishSetup(outcome));
    }

    /** Replaces the unpacked kit whenever a different APK is installed. */
    private void unpackResources() throws Exception {
        String installed = Long.toString(
                getPackageManager().getPackageInfo(getPackageName(), 0).lastUpdateTime);
        File root = new File(getFilesDir(), RESOURCES);
        File stamp = new File(getFilesDir(), RESOURCES + ".installed");
        if (root.isDirectory() && stamp.isFile()
                && installed.equals(new String(Files.readAllBytes(stamp.toPath()), StandardCharsets.UTF_8))) {
            return;
        }
        delete(root);
        copy(getAssets(), RESOURCES, root);
        Files.write(stamp.toPath(), installed.getBytes(StandardCharsets.UTF_8));
    }

    private static void copy(AssetManager assets, String path, File target) throws IOException {
        String[] children = assets.list(path);
        if (children == null || children.length == 0) {
            try (InputStream source = assets.open(path); OutputStream sink = new FileOutputStream(target)) {
                byte[] buffer = new byte[65536];
                for (int count; (count = source.read(buffer)) != -1; ) sink.write(buffer, 0, count);
            }
            return;
        }
        if (!target.isDirectory() && !target.mkdirs()) throw new IOException("cannot create " + target);
        for (String child : children) copy(assets, path + "/" + child, new File(target, child));
    }

    private static void delete(File file) {
        File[] children = file.listFiles();
        if (children != null) for (File child : children) delete(child);
        file.delete();
    }

    /** Called by the native setup on its worker thread; blocks until the user answers. */
    public boolean requestConsent(String title, String body) throws InterruptedException {
        CountDownLatch answer = new CountDownLatch(1);
        AtomicBoolean accepted = new AtomicBoolean();
        consentAnswer = answer;
        runOnUiThread(() -> {
            if (isFinishing() || isDestroyed()) { answer.countDown(); return; }
            screen.prompt(title, body, "Accept and download", () -> {
                accepted.set(true);
                answer.countDown();
            }, "Cancel", answer::countDown);
        });
        answer.await();
        return accepted.get();
    }

    /**
     * Called by the native setup on its worker thread; returns {@link PlatformTrust#pem} for the
     * translated local server's TLS and throws when the platform trust cannot be exported.
     */
    public String platformTrustPem() throws GeneralSecurityException {
        return PlatformTrust.pem();
    }

    /** Called by the native setup on its worker thread with one JSON progress update. */
    public void showProgress(String update) {
        runOnUiThread(() -> {
            if (!isFinishing() && !isDestroyed()) screen.progress(update);
        });
    }

    private void finishSetup(String error) {
        if (isFinishing() || isDestroyed()) return;
        if (error == null) {
            startActivity(new Intent(this, CinnabarActivity.class));
            finish();
        } else if (error.isEmpty()) {
            finish();
        } else {
            screen.prompt("@PRODUCT_NAME@ setup failed", error, "Retry", this::start, "Close", this::finish);
        }
    }

    @Override protected void onDestroy() {
        cancel(attempt);
        CountDownLatch answer = consentAnswer;
        if (answer != null) answer.countDown();
        super.onDestroy();
    }
}
