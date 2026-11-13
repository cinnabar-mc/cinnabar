package @APPLICATION_ID@;

import android.app.Activity;
import android.app.AlertDialog;
import android.app.ApplicationExitInfo;
import android.content.ContentResolver;
import android.content.ContentValues;
import android.content.Context;
import android.net.Uri;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.provider.MediaStore;
import android.widget.Toast;
import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.RandomAccessFile;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.zip.ZipEntry;
import java.util.zip.ZipOutputStream;

/**
 * Exports only this app's exit or session details, client log, adapter record and OS trace,
 * without storage permissions or uploads.
 */
final class CrashReport {
    /** The newest bytes of an attached file that a report keeps. */
    static final long ATTACHMENT_BYTES = 32L << 20;
    /** The first Android version with the Downloads collection that reports are saved to. */
    static final int DOWNLOADS_SDK = 29;
    /** Set while a session report is being written, so repeated requests write one file. */
    private static final AtomicBoolean SAVING_SESSION = new AtomicBoolean();

    /** Writes a report's ZIP to the stream Downloads opened for it. */
    private interface Body {
        void write(OutputStream output) throws IOException;
    }

    /** Writes a report in Downloads on a worker, keeping the crash dialog open for retry. */
    static void save(Activity activity, AlertDialog dialog, ApplicationExitInfo exit, String details) {
        dialog.getButton(AlertDialog.BUTTON_NEGATIVE).setEnabled(false);
        dialog.getButton(AlertDialog.BUTTON_POSITIVE).setEnabled(false);
        ContentResolver resolver = activity.getApplicationContext().getContentResolver();
        File[] attachments = attachments(activity.getFilesDir());
        new Thread(() -> {
            String result;
            try {
                publish(resolver, "@PRODUCT_NAME@-crash-" + exit.getTimestamp() + ".zip", output -> {
                    try (InputStream trace = exit.getTraceInputStream()) {
                        write(output, details, trace, Build.VERSION.SDK_INT >= 31
                                && exit.getReason() == ApplicationExitInfo.REASON_CRASH_NATIVE,
                                attachments);
                    }
                });
                result = "Crash report saved to Downloads";
            } catch (IOException | RuntimeException error) {
                result = "Cannot save crash report: " + error.getMessage();
            }
            String message = result;
            activity.runOnUiThread(() -> {
                if (activity.isDestroyed()) return;
                dialog.getButton(AlertDialog.BUTTON_NEGATIVE).setEnabled(true);
                dialog.getButton(AlertDialog.BUTTON_POSITIVE).setEnabled(true);
                Toast.makeText(activity, message, Toast.LENGTH_LONG).show();
            });
        }, "crash-report").start();
    }

    /** Whether this Android version can save a session report; native code asks before offering it. */
    static boolean canSaveSession() {
        return Build.VERSION.SDK_INT >= DOWNLOADS_SDK;
    }

    /**
     * Saves a report of the running session to Downloads: device and build details, recent
     * exits and the client log so far. Native code calls this on any thread with the
     * application context; a worker writes the file and a toast reports the result. A request
     * made while an earlier one is still being written is dropped.
     */
    static void saveSession(Context context) {
        Context application = context.getApplicationContext();
        if (!canSaveSession()) {
            toast(application, "Saving a diagnostic report needs Android 10 or newer");
            return;
        }
        if (!SAVING_SESSION.compareAndSet(false, true)) return;
        ContentResolver resolver = application.getContentResolver();
        File[] attachments = attachments(application.getFilesDir());
        new Thread(() -> {
            String result;
            try {
                long now = System.currentTimeMillis();
                String details = StartupDiagnostics.sessionDetails(application, now);
                publish(resolver, "@PRODUCT_NAME@-report-" + now + ".zip",
                        output -> writeSession(output, details, attachments));
                result = "Diagnostic report saved to Downloads";
            } catch (IOException | RuntimeException error) {
                result = "Cannot save diagnostic report: " + error.getMessage();
            } finally {
                SAVING_SESSION.set(false);
            }
            toast(application, result);
        }, "session-report").start();
    }

    /** The client's files that every report attaches, under the app's files directory. */
    private static File[] attachments(File files) {
        return new File[] {
            new File(files, StartupDiagnostics.CLIENT_LOG),
            new File(files, StartupDiagnostics.ADAPTER_RECORD),
        };
    }

    /** Shows `message` from any thread; a toast needs the main thread's looper. */
    private static void toast(Context application, String message) {
        new Handler(Looper.getMainLooper()).post(() ->
                Toast.makeText(application, message, Toast.LENGTH_LONG).show());
    }

    /**
     * Publishes the ZIP that `body` writes as `name` in Downloads. The entry stays pending
     * until it is complete, and a failed write removes it.
     */
    private static void publish(ContentResolver resolver, String name, Body body) throws IOException {
        Uri target = null;
        try {
            ContentValues values = new ContentValues();
            values.put(MediaStore.Downloads.DISPLAY_NAME, name);
            values.put(MediaStore.Downloads.MIME_TYPE, "application/zip");
            values.put(MediaStore.Downloads.IS_PENDING, 1);
            target = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values);
            if (target == null) throw new IOException("Downloads is unavailable");
            try (OutputStream output = resolver.openOutputStream(target)) {
                if (output == null) throw new IOException("Cannot open the report");
                body.write(output);
            }
            values.clear();
            values.put(MediaStore.Downloads.IS_PENDING, 0);
            if (resolver.update(target, values, null, null) != 1)
                throw new IOException("Cannot publish the report in Downloads");
            target = null;
        } finally {
            if (target != null) {
                try { resolver.delete(target, null, null); }
                catch (RuntimeException error) {
                    android.util.Log.w("@PRODUCT_NAME@", "Cannot remove incomplete report", error);
                }
            }
        }
    }

    /**
     * Preserves the OS trace bytes verbatim; a missing trace is explicitly recorded. Each
     * attachment that exists follows under its own file name, so lines the details shorten
     * stay whole in the report.
     */
    static void write(OutputStream output, String details, InputStream trace, boolean nativeCrash,
            File... attachments) throws IOException {
        try (ZipOutputStream zip = new ZipOutputStream(output)) {
            text(zip, "details.txt", details);
            byte[] buffer = new byte[65536];
            if (trace == null) {
                text(zip, "trace-unavailable.txt", "Android did not retain a trace for this process exit.\n");
            } else {
                zip.putNextEntry(new ZipEntry(nativeCrash ? "tombstone.pb" : "trace.txt"));
                for (int count; (count = trace.read(buffer)) != -1; ) zip.write(buffer, 0, count);
                zip.closeEntry();
            }
            for (File attachment : attachments) attach(zip, attachment, buffer);
        }
    }

    /**
     * A report of a running session: its details and each attachment that exists. No process
     * exit is being reported, so it has no trace entry.
     */
    static void writeSession(OutputStream output, String details, File... attachments)
            throws IOException {
        try (ZipOutputStream zip = new ZipOutputStream(output)) {
            text(zip, "details.txt", details);
            byte[] buffer = new byte[65536];
            for (File attachment : attachments) attach(zip, attachment, buffer);
        }
    }

    /** Adds `text` as the UTF-8 entry `name`. */
    private static void text(ZipOutputStream zip, String name, String text) throws IOException {
        zip.putNextEntry(new ZipEntry(name));
        zip.write(text.getBytes(StandardCharsets.UTF_8));
        zip.closeEntry();
    }

    /**
     * Adds the newest {@link #ATTACHMENT_BYTES} of a file, from its first whole line when the
     * start is cut; a file that cannot be opened is left out.
     */
    private static void attach(ZipOutputStream zip, File file, byte[] buffer) throws IOException {
        RandomAccessFile source;
        try {
            source = new RandomAccessFile(file, "r");
        } catch (IOException error) {
            return;
        }
        try {
            source.seek(wholeLineStart(source, Math.max(0, source.length() - ATTACHMENT_BYTES), buffer));
            zip.putNextEntry(new ZipEntry(file.getName()));
            for (int count; (count = source.read(buffer)) != -1; ) zip.write(buffer, 0, count);
            zip.closeEntry();
        } finally {
            source.close();
        }
    }

    /**
     * The first line start at or after `cut`, so a file cut there begins with a whole line.
     * A final line that itself starts before `cut` is kept from `cut`.
     */
    static long wholeLineStart(RandomAccessFile source, long cut, byte[] buffer) throws IOException {
        if (cut == 0) return 0;
        // A newline just before the cut means the cut already falls on a line start.
        long offset = cut - 1;
        source.seek(offset);
        for (int count; (count = source.read(buffer)) != -1; offset += count) {
            for (int index = 0; index < count; index++) {
                if (buffer[index] != '\n') continue;
                long line = offset + index + 1;
                return line < source.length() ? line : cut;
            }
        }
        return cut;
    }
}
