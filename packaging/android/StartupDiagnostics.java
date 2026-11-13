package @APPLICATION_ID@;

import android.app.Activity;
import android.app.ActivityManager;
import android.app.AlertDialog;
import android.app.ApplicationExitInfo;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.os.Build;
import android.widget.Toast;
import java.io.File;
import java.io.IOException;
import java.io.RandomAccessFile;
import java.nio.ByteBuffer;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.regex.Pattern;

/** Makes an unexpected exit visible on the next launch without needing USB debugging. */
final class StartupDiagnostics {
    /** The most UTF-8 bytes shown for each of the start and the end of the client log. */
    static final int LOG_TAIL_BYTES = 32768;
    /** Raw log bytes read for each of those windows, before long lines are shortened. */
    static final int LOG_SCAN_BYTES = 1 << 20;
    /** Longer lines, such as one-line profiler JSON, are shortened in the details. */
    static final int LOG_LINE_CHARS = 2048;
    /** Characters kept from the start of a shortened line: its time, level and marker. */
    static final int LOG_LINE_HEAD_CHARS = 200;
    /** Characters kept from the end of a shortened line. */
    static final int LOG_LINE_TAIL_CHARS = 80;
    /** Recent process exits read from Android; those before the reported one are listed. */
    static final int EXIT_HISTORY = 8;
    /** The client's current session log, relative to the app's files directory. */
    static final String CLIENT_LOG = "data/logs/client.log";
    /** The GPU adapter the renderer last chose, written by the client at startup. */
    static final String ADAPTER_RECORD = "data/logs/adapter.json";
    /** Bounds the adapter record shown in the details; the client writes a few hundred bytes. */
    static final int ADAPTER_RECORD_BYTES = 4096;
    private static final String GENERIC_EXIT = "Android stopped @PRODUCT_NAME@ for an unknown reason.";
    private static final String NO_LOG = "No client log was written before the process stopped.";
    private static final Pattern LOG_TIMESTAMP = Pattern.compile(
            "^\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}(?:\\.\\d+)?Z\\s+");

    /** Offers retry and copyable diagnostics once for each unexpected main-process exit. */
    static boolean showPreviousFailure(Activity activity, Runnable retry) {
        if (Build.VERSION.SDK_INT < 30) return false;
        try {
            ActivityManager manager = activity.getSystemService(ActivityManager.class);
            String process = activity.getApplicationInfo().processName;
            List<ApplicationExitInfo> exits = new ArrayList<>();
            for (ApplicationExitInfo record : manager.getHistoricalProcessExitReasons(
                    activity.getPackageName(), 0, EXIT_HISTORY)) {
                if (process.equals(record.getProcessName())) exits.add(record);
            }
            if (exits.isEmpty()) return false;
            ApplicationExitInfo exit = exits.get(0);
            if (!unexpected(exit.getReason(), exit.getStatus())) return false;
            android.content.SharedPreferences preferences = activity.getPreferences(0);
            if (exit.getTimestamp() <= preferences.getLong("reported-exit", 0)) return false;
            preferences.edit().putLong("reported-exit", exit.getTimestamp()).apply();
            StringBuilder earlier = new StringBuilder();
            for (ApplicationExitInfo record : exits.subList(1, exits.size())) {
                earlier.append(exitLine(record.getTimestamp(), record.getReason(),
                        record.getStatus(), record.getDescription())).append('\n');
            }
            File files = activity.getFilesDir();
            String details = header(deviceLine(), socLine(), totalMemory(manager),
                            String.join(", ", Build.SUPPORTED_ABIS), pageSize(),
                            adapterRecord(new File(files, ADAPTER_RECORD)))
                    + "\n\nExit reason: " + exit.getReason() + ", status: " + exit.getStatus()
                    + "\n" + exitCause(exit.getReason(), exit.getStatus())
                    + "\n" + exit.getDescription() + "\n"
                    + "Process: " + exit.getProcessName() + " (" + exit.getPid() + ")\n"
                    + "Exit timestamp: " + exit.getTimestamp() + " ("
                    + java.time.Instant.ofEpochMilli(exit.getTimestamp()) + ")\n"
                    + (earlier.length() == 0 ? "" : "Earlier exits, newest first:\n" + earlier)
                    + "\n" + logContext(new File(files, CLIENT_LOG));
            AlertDialog dialog = new AlertDialog.Builder(activity)
                    .setTitle("@PRODUCT_NAME@ stopped unexpectedly")
                    .setMessage("Save report puts these details, the full client log and any Android crash trace in Downloads.\n\n" + details)
                    .setPositiveButton("Retry", (which, button) -> retry.run())
                    .setNegativeButton("Save report", null)
                    .setNeutralButton("Copy details", null)
                    .setOnCancelListener(which -> activity.finish())
                    .create();
            dialog.setOnShowListener(which -> {
                dialog.getButton(AlertDialog.BUTTON_NEUTRAL).setOnClickListener(view -> {
                    activity.getSystemService(ClipboardManager.class)
                            .setPrimaryClip(ClipData.newPlainText("@PRODUCT_NAME@ diagnostics", details));
                    Toast.makeText(activity, "Diagnostic details copied", Toast.LENGTH_SHORT).show();
                });
                dialog.getButton(AlertDialog.BUTTON_NEGATIVE).setOnClickListener(view ->
                        CrashReport.save(activity, dialog, exit, details));
            });
            dialog.show();
            return true;
        } catch (RuntimeException error) {
            android.util.Log.w("@PRODUCT_NAME@", "Cannot read previous process exit", error);
            return false;
        }
    }

    /**
     * Identifies the build and device at the top of the details. Nightly builds reuse the last
     * release's version, so the commit and channel tell them apart. The adapter record names
     * the GPU and driver even when the log's startup lines are lost. Nothing here names an
     * account, a server or a player.
     */
    static String header(String device, String soc, long memoryBytes, String abis, long pageBytes,
            String adapter) {
        return "@PRODUCT_NAME@ @VERSION_NAME@, build @BUILD_COMMIT@ (@RELEASE_CHANNEL@ channel)\n"
                + device + "\n"
                + "SoC: " + soc + "\n"
                + "RAM: " + (memoryBytes >> 20) + " MiB, ABI: " + abis + ", page size: " + pageBytes + "\n"
                + (adapter.isEmpty()
                        ? "GPU adapter: not recorded; the renderer had not chosen one on this install."
                        : "GPU adapter chosen at the last renderer start:\n" + adapter);
    }

    /** The device's maker, model and Android version. */
    private static String deviceLine() {
        return Build.MANUFACTURER + " " + Build.MODEL + ", Android " + Build.VERSION.RELEASE
                + " (API " + Build.VERSION.SDK_INT + ")";
    }

    /** The system-on-chip, which Android names from API 31, with the older board identifiers. */
    private static String socLine() {
        String chip = Build.VERSION.SDK_INT >= 31
                ? Build.SOC_MANUFACTURER + " " + Build.SOC_MODEL : "not reported before Android 12";
        return chip + " (hardware " + Build.HARDWARE + ", board " + Build.BOARD + ")";
    }

    /** Physical memory visible to Android, in bytes. */
    private static long totalMemory(ActivityManager manager) {
        ActivityManager.MemoryInfo memory = new ActivityManager.MemoryInfo();
        manager.getMemoryInfo(memory);
        return memory.totalMem;
    }

    /** The kernel's page size in bytes, or -1 when Android cannot report it. */
    private static long pageSize() {
        return android.system.Os.sysconf(android.system.OsConstants._SC_PAGESIZE);
    }

    /** Reads the client's adapter record as written; empty when no renderer start recorded one. */
    static String adapterRecord(File file) {
        try (RandomAccessFile source = new RandomAccessFile(file, "r")) {
            return decode(read(source, 0, (int) Math.min(source.length(), ADAPTER_RECORD_BYTES))).trim();
        } catch (IOException error) {
            return "";
        }
    }

    /** One earlier exit: its UTC time, Android's numeric reason and status, and their meaning. */
    static String exitLine(long timestamp, int reason, int status, String description) {
        return "  " + java.time.Instant.ofEpochMilli(timestamp) + " reason " + reason + ", status " + status
                + (unexpected(reason, status) ? ": " + exitCause(reason, status) : "")
                + (description == null || description.isEmpty() ? "" : " (" + description + ")");
    }

    /** Only exits with an identified unexpected cause need a recovery dialog. */
    static boolean unexpected(int reason, int status) {
        return !GENERIC_EXIT.equals(exitCause(reason, status));
    }

    /** Names every admitted failure; intentional shutdown and unknown reasons use generic text. */
    static String exitCause(int reason, int status) {
        switch (reason) {
            case ApplicationExitInfo.REASON_CRASH:
                return "@PRODUCT_NAME@ stopped because of an application crash.";
            case ApplicationExitInfo.REASON_CRASH_NATIVE:
                return "@PRODUCT_NAME@ stopped because of a native crash.";
            case ApplicationExitInfo.REASON_LOW_MEMORY:
                return "Android closed @PRODUCT_NAME@ because the device ran low on memory.";
            case ApplicationExitInfo.REASON_ANR:
                return "Android closed @PRODUCT_NAME@ because it stopped responding.";
            case ApplicationExitInfo.REASON_EXIT_SELF:
                if (status != 0) return "@PRODUCT_NAME@ exited after an error.";
                return GENERIC_EXIT;
            case ApplicationExitInfo.REASON_SIGNALED:
                if (status != android.system.OsConstants.SIGKILL)
                    return "@PRODUCT_NAME@ stopped because it received a system signal.";
                return GENERIC_EXIT;
            default:
                return GENERIC_EXIT;
        }
    }

    /**
     * The log for the details: all of it when it fits two windows once readable, else its
     * start, which holds the build and startup context, and its end.
     */
    static String logContext(File file) {
        try (RandomAccessFile source = new RandomAccessFile(file, "r")) {
            long length = source.length();
            String start;
            String end;
            if (length <= 2L * LOG_SCAN_BYTES) {
                String all = readable(decode(read(source, 0, (int) length)));
                if (all.getBytes(StandardCharsets.UTF_8).length <= 2 * LOG_TAIL_BYTES) return all;
                start = boundedHead(all);
                end = boundedTail(all);
            } else {
                start = boundedHead(readable(decode(read(source, 0, LOG_SCAN_BYTES))));
                end = boundedTail(readable(decode(
                        read(source, length - LOG_SCAN_BYTES, LOG_SCAN_BYTES))));
            }
            return "Client log start (build and startup context):\n" + start
                    + "\n\nClient log tail (intervening records omitted):\n" + end;
        } catch (IOException error) {
            return NO_LOG;
        }
    }

    /** Reads a bounded, readable tail of the app's own log; absent logs never prevent a retry. */
    static String logTail(File file) {
        try (RandomAccessFile source = new RandomAccessFile(file, "r")) {
            int count = (int) Math.min(source.length(), LOG_SCAN_BYTES);
            return boundedTail(readable(decode(read(source, source.length() - count, count))));
        } catch (IOException error) {
            return NO_LOG;
        }
    }

    /** Reads `count` bytes of the log starting at `offset`. */
    private static byte[] read(RandomAccessFile source, long offset, int count) throws IOException {
        byte[] bytes = new byte[count];
        source.seek(offset);
        source.readFully(bytes);
        return bytes;
    }

    /** Decodes a window of the log, dropping a character that either edge of the window split. */
    private static String decode(byte[] bytes) throws IOException {
        int start = 0;
        while (start < bytes.length && (bytes[start] & 0xc0) == 0x80) start++;
        return StandardCharsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.IGNORE)
                .onUnmappableCharacter(CodingErrorAction.IGNORE)
                .decode(ByteBuffer.wrap(bytes, start, bytes.length - start)).toString();
    }

    /** What the details show of raw log text: long lines shortened, then repeats collapsed. */
    static String readable(String log) {
        String[] lines = log.split("\\n", -1);
        for (int index = 0; index < lines.length; index++) lines[index] = shortenLine(lines[index]);
        return compactLog(lines);
    }

    /**
     * Keeps the start and end of a line longer than {@link #LOG_LINE_CHARS}. One-line profiler
     * JSON otherwise fills the bounded details and pushes out the records around a failure;
     * the saved report's client log keeps every line whole.
     */
    static String shortenLine(String line) {
        if (line.length() <= LOG_LINE_CHARS) return line;
        int head = LOG_LINE_HEAD_CHARS;
        int tail = line.length() - LOG_LINE_TAIL_CHARS;
        // A cut inside a surrogate pair would leave half a character on each side.
        if (Character.isHighSurrogate(line.charAt(head - 1))) head--;
        if (Character.isLowSurrogate(line.charAt(tail))) tail++;
        return line.substring(0, head) + " ... [" + (tail - head)
                + " characters omitted; the saved report keeps the full line] ... " + line.substring(tail);
    }

    /** Collapses adjacent identical records while retaining the first timestamp and log order. */
    private static String compactLog(String[] lines) {
        StringBuilder output = new StringBuilder();
        for (int first = 0; first < lines.length; ) {
            String key = LOG_TIMESTAMP.matcher(lines[first]).replaceFirst("");
            int next = first + 1;
            while (!key.isEmpty() && next < lines.length
                    && key.equals(LOG_TIMESTAMP.matcher(lines[next]).replaceFirst(""))) next++;
            output.append(lines[first]);
            if (next - first > 1) output.append(" (repeated ").append(next - first).append(" times)");
            if (next < lines.length) output.append('\n');
            first = next;
        }
        return output.toString();
    }

    /** Keeps the oldest bytes, ending on a whole line when one fits and never inside a character. */
    private static String boundedHead(String text) {
        byte[] bytes = text.getBytes(StandardCharsets.UTF_8);
        if (bytes.length <= LOG_TAIL_BYTES) return text;
        int end = LOG_TAIL_BYTES;
        while (end > 0 && (bytes[end] & 0xc0) == 0x80) end--;
        for (int index = end - 1; index > 0; index--) {
            if (bytes[index] == '\n') {
                end = index;
                break;
            }
        }
        return new String(bytes, 0, end, StandardCharsets.UTF_8);
    }

    /** Keeps the newest bytes, starting on a whole line when one starts early and never inside a character. */
    private static String boundedTail(String text) {
        byte[] bytes = text.getBytes(StandardCharsets.UTF_8);
        if (bytes.length <= LOG_TAIL_BYTES) return text;
        int start = bytes.length - LOG_TAIL_BYTES;
        while (start < bytes.length && (bytes[start] & 0xc0) == 0x80) start++;
        for (int index = start; index < start + LOG_TAIL_BYTES / 2; index++) {
            if (bytes[index] == '\n') {
                start = index + 1;
                break;
            }
        }
        return new String(bytes, start, bytes.length - start, StandardCharsets.UTF_8);
    }
}
