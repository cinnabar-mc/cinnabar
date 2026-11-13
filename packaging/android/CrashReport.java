package @APPLICATION_ID@;

import android.app.Activity;
import android.app.AlertDialog;
import android.app.ApplicationExitInfo;
import android.content.ContentResolver;
import android.content.ContentValues;
import android.net.Uri;
import android.os.Build;
import android.provider.MediaStore;
import android.widget.Toast;
import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.RandomAccessFile;
import java.nio.charset.StandardCharsets;
import java.util.zip.ZipEntry;
import java.util.zip.ZipOutputStream;

/**
 * Exports only this app's exit details, client log, adapter record and OS trace, without
 * storage permissions or uploads.
 */
final class CrashReport {
    /** The newest bytes of an attached file that a report keeps. */
    static final long ATTACHMENT_BYTES = 32L << 20;

    /** Writes a report in Downloads on a worker, keeping the crash dialog open for retry. */
    static void save(Activity activity, AlertDialog dialog, ApplicationExitInfo exit, String details) {
        dialog.getButton(AlertDialog.BUTTON_NEGATIVE).setEnabled(false);
        dialog.getButton(AlertDialog.BUTTON_POSITIVE).setEnabled(false);
        ContentResolver resolver = activity.getApplicationContext().getContentResolver();
        File files = activity.getFilesDir();
        File[] attachments = {
            new File(files, StartupDiagnostics.CLIENT_LOG),
            new File(files, StartupDiagnostics.ADAPTER_RECORD),
        };
        new Thread(() -> {
            Uri target = null;
            String result;
            try {
                ContentValues values = new ContentValues();
                values.put(MediaStore.Downloads.DISPLAY_NAME, "@PRODUCT_NAME@-crash-" + exit.getTimestamp() + ".zip");
                values.put(MediaStore.Downloads.MIME_TYPE, "application/zip");
                values.put(MediaStore.Downloads.IS_PENDING, 1);
                target = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values);
                if (target == null) throw new IOException("Downloads is unavailable");
                try (OutputStream output = resolver.openOutputStream(target);
                        InputStream trace = exit.getTraceInputStream()) {
                    if (output == null) throw new IOException("Cannot open the report");
                    write(output, details, trace, Build.VERSION.SDK_INT >= 31
                            && exit.getReason() == ApplicationExitInfo.REASON_CRASH_NATIVE,
                            attachments);
                }
                values.clear();
                values.put(MediaStore.Downloads.IS_PENDING, 0);
                if (resolver.update(target, values, null, null) != 1)
                    throw new IOException("Cannot publish the report in Downloads");
                target = null;
                result = "Crash report saved to Downloads";
            } catch (IOException | RuntimeException error) {
                result = "Cannot save crash report: " + error.getMessage();
            } finally {
                if (target != null) {
                    try { resolver.delete(target, null, null); }
                    catch (RuntimeException error) {
                        android.util.Log.w("@PRODUCT_NAME@", "Cannot remove incomplete crash report", error);
                    }
                }
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

    /**
     * Preserves the OS trace bytes verbatim; a missing trace is explicitly recorded. Each
     * attachment that exists follows under its own file name, so lines the details shorten
     * stay whole in the report.
     */
    static void write(OutputStream output, String details, InputStream trace, boolean nativeCrash,
            File... attachments) throws IOException {
        try (ZipOutputStream zip = new ZipOutputStream(output)) {
            zip.putNextEntry(new ZipEntry("details.txt"));
            zip.write(details.getBytes(StandardCharsets.UTF_8));
            zip.closeEntry();
            byte[] buffer = new byte[65536];
            if (trace == null) {
                zip.putNextEntry(new ZipEntry("trace-unavailable.txt"));
                zip.write("Android did not retain a trace for this process exit.\n".getBytes(StandardCharsets.UTF_8));
            } else {
                zip.putNextEntry(new ZipEntry(nativeCrash ? "tombstone.pb" : "trace.txt"));
                for (int count; (count = trace.read(buffer)) != -1; ) zip.write(buffer, 0, count);
            }
            zip.closeEntry();
            for (File attachment : attachments) attach(zip, attachment, buffer);
        }
    }

    /** Adds the newest {@link #ATTACHMENT_BYTES} of a file; a file that cannot be opened is left out. */
    private static void attach(ZipOutputStream zip, File file, byte[] buffer) throws IOException {
        RandomAccessFile source;
        try {
            source = new RandomAccessFile(file, "r");
        } catch (IOException error) {
            return;
        }
        try {
            source.seek(Math.max(0, source.length() - ATTACHMENT_BYTES));
            zip.putNextEntry(new ZipEntry(file.getName()));
            for (int count; (count = source.read(buffer)) != -1; ) zip.write(buffer, 0, count);
            zip.closeEntry();
        } finally {
            source.close();
        }
    }
}
