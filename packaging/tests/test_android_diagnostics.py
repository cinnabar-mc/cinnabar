"""Compile the Android activities and exercise platform-independent exit diagnostics."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
PINS = json.loads((ROOT / "packaging/android/runtime.json").read_text())


class AndroidDiagnosticsTests(unittest.TestCase):
    def compile_activities(self, output):
        """Compile rendered production activities and any Java fixture in the output directory."""
        sdk = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
        if not sdk or not shutil.which("javac"):
            self.skipTest("missing fixture: Android SDK and Java toolchain")
        android_jar = Path(sdk) / "platforms" / f'android-{PINS["target_sdk"]}' / "android.jar"
        if not android_jar.is_file():
            self.skipTest("missing fixture: pinned Android SDK platform")
        classes = output / "classes"
        classes.mkdir()
        rendered = subprocess.run([
            "bash", "-euc", '''
repo_root="$1"
out="$2"
pin() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$repo_root/packaging/android/runtime.json" "$1"; }
client_library=fixture_client
product=Fixture
version_code=1
version=0.0.1
source "$repo_root/packaging/android/render-template.sh"
for file in "$repo_root"/packaging/android/*.java; do
    render "$file" "$out/$(basename "$file")"
done
''', "android-java-test", str(ROOT), str(output),
        ], text=True, capture_output=True, env={
            **os.environ, "CINNABAR_BUILD_COMMIT": "0123abc", "CINNABAR_RELEASE_CHANNEL": "nightly",
        })
        self.assertEqual(rendered.returncode, 0, rendered.stderr)
        compiled = subprocess.run([
            "javac", "--release", "8", "-nowarn", "-classpath", str(android_jar),
            "-d", str(classes), *map(str, output.glob("*.java")),
        ], text=True, capture_output=True)
        self.assertEqual(compiled.returncode, 0, compiled.stderr)
        return classes, android_jar

    def test_build_identity_renders_literally(self):
        """Characters sed interprets in a replacement reach the rendered source unchanged."""
        with tempfile.TemporaryDirectory() as directory:
            template = Path(directory) / "identity.txt"
            template.write_text("@BUILD_COMMIT@ @RELEASE_CHANNEL@\n")
            rendered = subprocess.run([
                "bash", "-euc", '''
repo_root="$1"
pin() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$repo_root/packaging/android/runtime.json" "$1"; }
client_library=fixture_client
product=Fixture
version_code=1
version=0.0.1
source "$repo_root/packaging/android/render-template.sh"
render "$2" "$2.out"
''', "android-render-test", str(ROOT), str(template),
            ], text=True, capture_output=True, env={
                **os.environ, "CINNABAR_BUILD_COMMIT": "a&b", "CINNABAR_RELEASE_CHANNEL": "night|ly\\x",
            })
            self.assertEqual(rendered.returncode, 0, rendered.stderr)
            self.assertEqual(Path(f"{template}.out").read_text(), "a&b night|ly\\x\n")

    def test_activities_compile(self):
        """Production activities must compile together, including their lifecycle overrides."""
        with tempfile.TemporaryDirectory() as directory:
            self.compile_activities(Path(directory))

    def test_failed_exits_and_bounded_log_tails_remain_readable(self):
        """Unexpected exits produce diagnostics; normal exits and missing logs remain harmless."""
        if not shutil.which("java"):
            self.skipTest("missing fixture: Java runtime")
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            fixture = output / "DiagnosticsFixture.java"
            fixture.write_text("package " + PINS["application_id"] + ";\n" + '''
import android.app.ApplicationExitInfo;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.Arrays;
import java.util.zip.ZipInputStream;
public final class DiagnosticsFixture {
    public static void main(String[] args) throws Exception {
        for (int reason : new int[] {ApplicationExitInfo.REASON_CRASH,
                ApplicationExitInfo.REASON_CRASH_NATIVE, ApplicationExitInfo.REASON_ANR,
                ApplicationExitInfo.REASON_LOW_MEMORY}) {
            if (!StartupDiagnostics.unexpected(reason, 0)) throw new AssertionError(reason);
        }
        if (!StartupDiagnostics.unexpected(ApplicationExitInfo.REASON_EXIT_SELF, 1))
            throw new AssertionError("failed startup exit was ignored");
        for (int reason : new int[] {ApplicationExitInfo.REASON_EXIT_SELF,
                ApplicationExitInfo.REASON_USER_REQUESTED, ApplicationExitInfo.REASON_PACKAGE_UPDATED, -1}) {
            if (StartupDiagnostics.unexpected(reason, 0)) throw new AssertionError(reason);
        }
        String generic = StartupDiagnostics.exitCause(-1, 0);
        for (java.lang.reflect.Field field : ApplicationExitInfo.class.getFields()) {
            if (!field.getName().startsWith("REASON_")) continue;
            int reason = field.getInt(null);
            for (int status : new int[] {0, 1, android.system.OsConstants.SIGKILL}) {
                if (StartupDiagnostics.unexpected(reason, status)
                        && generic.equals(StartupDiagnostics.exitCause(reason, status)))
                    throw new AssertionError("missing cause for " + field.getName());
            }
        }
        if (!StartupDiagnostics.exitCause(ApplicationExitInfo.REASON_LOW_MEMORY, 0)
                .contains("ran low on memory")) throw new AssertionError("memory cause missing");
        if (!StartupDiagnostics.unexpected(ApplicationExitInfo.REASON_SIGNALED, 1)
                || StartupDiagnostics.unexpected(ApplicationExitInfo.REASON_SIGNALED,
                        android.system.OsConstants.SIGKILL))
            throw new AssertionError("signal exit classification changed");
        File log = new File(args[0]);
        String input = "discarded prefix" + new String(new char[100000]).replace('\\0', 'x') + "end-marker";
        Files.write(log.toPath(), input.getBytes(StandardCharsets.UTF_8));
        String tail = StartupDiagnostics.logTail(log);
        if (!tail.endsWith("end-marker") || tail.length() >= input.length())
            throw new AssertionError("log tail lost its end or was not bounded");
        String header = "fixture session build=abc123 adapter=fixture\\n";
        Files.write(log.toPath(), (header + input).getBytes(StandardCharsets.UTF_8));
        String context = StartupDiagnostics.logContext(log);
        if (!context.contains(header) || !context.endsWith("end-marker")
                || context.getBytes(StandardCharsets.UTF_8).length > 2 * StartupDiagnostics.LOG_TAIL_BYTES + 200)
            throw new AssertionError("startup context or bounded tail lost");
        Files.write(log.toPath(), "short log".getBytes(StandardCharsets.UTF_8));
        if (!"short log".equals(StartupDiagnostics.logContext(log)))
            throw new AssertionError("short logs were duplicated");
        String records = "2026-10-24T10:00:00.001Z ERROR cursor: unavailable\\n"
                + "2026-10-24T10:00:00.002Z ERROR cursor: unavailable\\n"
                + "2026-10-24T10:00:00.003Z ERROR cursor: unavailable\\n"
                + "2026-10-24T10:00:00.004Z INFO client: useful history\\n"
                + "2026-10-24T10:00:00.005Z ERROR cursor: unavailable\\n";
        Files.write(log.toPath(), records.getBytes(StandardCharsets.UTF_8));
        String compact = StartupDiagnostics.logTail(log);
        String expectedLog = "2026-10-24T10:00:00.001Z ERROR cursor: unavailable (repeated 3 times)\\n"
                + "2026-10-24T10:00:00.004Z INFO client: useful history\\n"
                + "2026-10-24T10:00:00.005Z ERROR cursor: unavailable\\n";
        if (!expectedLog.equals(compact)) throw new AssertionError(compact);
        String shortRuns = new String(new char[6000]).replace("\\0", "a\\na\\nb\\n");
        Files.write(log.toPath(), shortRuns.getBytes(StandardCharsets.UTF_8));
        if (StartupDiagnostics.logTail(log).getBytes(StandardCharsets.UTF_8).length > StartupDiagnostics.LOG_TAIL_BYTES)
            throw new AssertionError("repeat annotations exceeded the byte bound");
        String unicode = new String(new char[20000]).replace("\\0", "\\u20ac") + "end-marker";
        Files.write(log.toPath(), unicode.getBytes(StandardCharsets.UTF_8));
        tail = StartupDiagnostics.logTail(log);
        if (tail.getBytes(StandardCharsets.UTF_8).length > StartupDiagnostics.LOG_TAIL_BYTES || tail.contains("\\ufffd")
                || !tail.endsWith("end-marker")) throw new AssertionError("UTF-8 tail was damaged");
        context = StartupDiagnostics.logContext(log);
        if (context.contains("\\ufffd") || !context.endsWith("end-marker"))
            throw new AssertionError("UTF-8 startup context was damaged");
        Files.write(log.toPath(), "same\\nsame\\nother\\nsame".getBytes(StandardCharsets.UTF_8));
        if (!"same (repeated 2 times)\\nother\\nsame".equals(StartupDiagnostics.logTail(log)))
            throw new AssertionError("untimestamped records or final line lost");
        String blob = "2026-10-24T10:00:01.000Z  WARN render: CINNABAR_HITCH {"
                + new String(new char[20000]).replace('\\0', 'j') + "}";
        String shortened = StartupDiagnostics.shortenLine(blob);
        if (shortened.length() > 400 || !shortened.endsWith("}") || !shortened.contains("characters omitted")
                || !shortened.startsWith("2026-10-24T10:00:01.000Z  WARN render: CINNABAR_HITCH {"))
            throw new AssertionError(shortened);
        String ordinary = new String(new char[StartupDiagnostics.LOG_LINE_CHARS]).replace('\\0', 'k');
        if (!ordinary.equals(StartupDiagnostics.shortenLine(ordinary)))
            throw new AssertionError("an ordinary line was shortened");
        String pairs = StartupDiagnostics.shortenLine(
                "a" + new String(new char[3000]).replace("\\0", "\\ud83d\\ude00"));
        if (!pairs.equals(new String(pairs.getBytes(StandardCharsets.UTF_8), StandardCharsets.UTF_8)))
            throw new AssertionError("a shortened line split a surrogate pair");
        StringBuilder noisy = new StringBuilder("2026-10-24T10:00:00.000Z  INFO client: session start\\n");
        for (int i = 0; i < 40; i++)
            noisy.append(blob).append(i).append("\\n2026-10-24T10:00:02.000Z ERROR render: real record ")
                    .append(i).append("\\n");
        Files.write(log.toPath(), noisy.toString().getBytes(StandardCharsets.UTF_8));
        context = StartupDiagnostics.logContext(log);
        for (int i = 0; i < 40; i++) {
            if (!context.contains("real record " + i + "\\n"))
                throw new AssertionError("profiler lines pushed out record " + i);
        }
        if (!context.contains("session start") || !context.contains("CINNABAR_HITCH {jjj")
                || context.getBytes(StandardCharsets.UTF_8).length > 2 * StartupDiagnostics.LOG_TAIL_BYTES)
            throw new AssertionError("profiler lines were not shortened in the details");
        StringBuilder huge = new StringBuilder("first-record\\n");
        for (int i = 0; i < 160; i++) huge.append(blob).append("\\nrecord ").append(i).append("\\n");
        huge.append("last-record");
        if (huge.length() <= 2 * StartupDiagnostics.LOG_SCAN_BYTES)
            throw new AssertionError("fixture no longer exceeds both scan windows");
        Files.write(log.toPath(), huge.toString().getBytes(StandardCharsets.UTF_8));
        context = StartupDiagnostics.logContext(log);
        if (!context.startsWith("Client log start") || !context.contains("first-record\\n")
                || !context.contains("record 0\\n") || !context.contains("record 159\\n")
                || !context.endsWith("last-record")
                || context.getBytes(StandardCharsets.UTF_8).length > 2 * StartupDiagnostics.LOG_TAIL_BYTES + 200)
            throw new AssertionError("a long log lost its start or end records");
        if (!StartupDiagnostics.logTail(log).endsWith("record 159\\nlast-record"))
            throw new AssertionError("log tail lost its last records");
        log.delete();
        if (StartupDiagnostics.logTail(log).isEmpty() || StartupDiagnostics.logContext(log).isEmpty())
            throw new AssertionError("missing log was not explained");
        String adapter = "{\\n  \\"name\\": \\"Fixture GPU\\",\\n  \\"driver_info\\": \\"build 1\\"\\n}";
        File record = new File(log.getParentFile(), "adapter.json");
        Files.write(record.toPath(), (adapter + "\\n").getBytes(StandardCharsets.UTF_8));
        if (!adapter.equals(StartupDiagnostics.adapterRecord(record))
                || !StartupDiagnostics.adapterRecord(new File(log.getParentFile(), "absent.json")).isEmpty())
            throw new AssertionError("adapter record was not read as written");
        String identity = StartupDiagnostics.header("Maker Model, Android 15 (API 35)",
                "Vendor Chip (hardware h, board b)", 8L << 30, "arm64-v8a", 16384, adapter);
        String[] identityLines = identity.split("\\n");
        if (!"Fixture 0.0.1, build 0123abc (nightly channel)".equals(identityLines[0])
                || !"Maker Model, Android 15 (API 35)".equals(identityLines[1])
                || !"SoC: Vendor Chip (hardware h, board b)".equals(identityLines[2])
                || !"RAM: 8192 MiB, ABI: arm64-v8a, page size: 16384".equals(identityLines[3])
                || !identity.endsWith(":\\n" + adapter))
            throw new AssertionError(identity);
        if (!StartupDiagnostics.header("d", "s", 0, "a", 4096, "").endsWith(
                "GPU adapter: not recorded; the renderer had not chosen one on this install."))
            throw new AssertionError("a missing adapter record was not explained");
        String marker = StartupDiagnostics.MEMORY_LINE;
        if (marker.isEmpty() || marker.contains("@")) throw new AssertionError(marker);
        Files.write(log.toPath(), ("2026-10-24T10:00:00.000Z  INFO client: session start\\n"
                + "2026-10-24T10:00:01.000Z  INFO reporter: " + marker + "startup rss=100M\\n"
                + "2026-10-24T10:00:02.000Z  INFO reporter: " + marker + "menu_ready rss=400M\\n"
                + "2026-10-24T10:00:02.500Z  WARN render: unrelated record\\n"
                + "2026-10-24T10:00:03.000Z  INFO reporter: " + marker + "world_entry rss=800M \\n"
                + marker + "periodic rss=900M").getBytes(StandardCharsets.UTF_8));
        String readings = StartupDiagnostics.memoryReadings(log);
        String expectedReadings = "Memory, last readings:\\n"
                + "2026-10-24T10:00:02.000Z " + marker + "menu_ready rss=400M\\n"
                + "2026-10-24T10:00:03.000Z " + marker + "world_entry rss=800M\\n"
                + marker + "periodic rss=900M\\n";
        if (!expectedReadings.equals(readings)) throw new AssertionError(readings);
        Files.write(log.toPath(), "a log without readings".getBytes(StandardCharsets.UTF_8));
        if (!"Memory: the log's last records hold no memory reading.\\n"
                .equals(StartupDiagnostics.memoryReadings(log)))
            throw new AssertionError("a log without memory readings was not explained");
        log.delete();
        if (!StartupDiagnostics.memoryReadings(log).isEmpty())
            throw new AssertionError("a missing log produced memory readings");
        String earlier = StartupDiagnostics.exitLine(0, ApplicationExitInfo.REASON_CRASH_NATIVE, 11, "crash");
        if (!earlier.contains("1970-01-01T00:00:00Z reason " + ApplicationExitInfo.REASON_CRASH_NATIVE
                + ", status 11: ") || !earlier.endsWith("native crash. (crash)"))
            throw new AssertionError(earlier);
        earlier = StartupDiagnostics.exitLine(0, ApplicationExitInfo.REASON_USER_REQUESTED, 0, null);
        if (!earlier.endsWith("reason " + ApplicationExitInfo.REASON_USER_REQUESTED + ", status 0"))
            throw new AssertionError(earlier);
        String memory = StartupDiagnostics.memoryLine(1024L << 20, 256L << 20, false);
        if (!("Memory when saved: 1024 MiB available, low-memory threshold 256 MiB, "
                + "Android reports low memory: no").equals(memory)
                || !StartupDiagnostics.memoryLine(0, 0, true).endsWith("low memory: yes"))
            throw new AssertionError(memory);
        String session = StartupDiagnostics.sessionDetails(identity, 0, memory, earlier + "\\n", "the log");
        if (!session.equals(identity
                + "\\n\\nSaved on request at 1970-01-01T00:00:00Z from a running session; "
                + "no exit is being reported.\\n" + memory + "\\nRecent exits, newest first:\\n"
                + earlier + "\\n\\nthe log"))
            throw new AssertionError(session);
        session = StartupDiagnostics.sessionDetails(identity, 0, memory, "", "the log");
        if (session.contains("Recent exits") || !session.endsWith(memory + "\\n\\nthe log"))
            throw new AssertionError("a session without retained exits listed some");
        byte[] trace = new byte[200000];
        for (int i = 0; i < trace.length; i++) trace[i] = (byte) i;
        for (boolean nativeCrash : new boolean[] {true, false}) {
            ByteArrayOutputStream report = new ByteArrayOutputStream();
            CrashReport.write(report, "exit details", new ByteArrayInputStream(trace), nativeCrash);
            try (ZipInputStream zip = new ZipInputStream(new ByteArrayInputStream(report.toByteArray()))) {
                if (!"details.txt".equals(zip.getNextEntry().getName())
                        || !"exit details".equals(new String(read(zip), StandardCharsets.UTF_8)))
                    throw new AssertionError("exit details missing");
                if (!(nativeCrash ? "tombstone.pb" : "trace.txt").equals(zip.getNextEntry().getName())
                        || !Arrays.equals(trace, read(zip)))
                    throw new AssertionError("Android trace was changed or truncated");
                if (zip.getNextEntry() != null) throw new AssertionError("unexpected report contents");
            }
        }
        ByteArrayOutputStream report = new ByteArrayOutputStream();
        CrashReport.write(report, "no trace", null, true);
        try (ZipInputStream zip = new ZipInputStream(new ByteArrayInputStream(report.toByteArray()))) {
            zip.getNextEntry();
            if (!"trace-unavailable.txt".equals(zip.getNextEntry().getName()) || read(zip).length == 0)
                throw new AssertionError("missing OS trace was not explained");
        }
        Files.write(log.toPath(), "whole log".getBytes(StandardCharsets.UTF_8));
        ByteArrayOutputStream attached = new ByteArrayOutputStream();
        CrashReport.write(attached, "details", null, false, log,
                new File(log.getParentFile(), "absent.json"), record);
        try (ZipInputStream zip = new ZipInputStream(new ByteArrayInputStream(attached.toByteArray()))) {
            zip.getNextEntry();
            zip.getNextEntry();
            if (!"client.log".equals(zip.getNextEntry().getName())
                    || !"whole log".equals(new String(read(zip), StandardCharsets.UTF_8)))
                throw new AssertionError("the full client log is missing from the report");
            if (!"adapter.json".equals(zip.getNextEntry().getName())
                    || !(adapter + "\\n").equals(new String(read(zip), StandardCharsets.UTF_8)))
                throw new AssertionError("the adapter record is missing from the report");
            if (zip.getNextEntry() != null) throw new AssertionError("an absent attachment was added");
        }
        ByteArrayOutputStream requested = new ByteArrayOutputStream();
        CrashReport.writeSession(requested, "session details", log,
                new File(log.getParentFile(), "absent.json"), record);
        try (ZipInputStream zip = new ZipInputStream(new ByteArrayInputStream(requested.toByteArray()))) {
            if (!"details.txt".equals(zip.getNextEntry().getName())
                    || !"session details".equals(new String(read(zip), StandardCharsets.UTF_8)))
                throw new AssertionError("session details missing");
            if (!"client.log".equals(zip.getNextEntry().getName())
                    || !"whole log".equals(new String(read(zip), StandardCharsets.UTF_8)))
                throw new AssertionError("a session report has no client log, or a trace entry before it");
            if (!"adapter.json".equals(zip.getNextEntry().getName())
                    || !(adapter + "\\n").equals(new String(read(zip), StandardCharsets.UTF_8)))
                throw new AssertionError("the adapter record is missing from the session report");
            if (zip.getNextEntry() != null) throw new AssertionError("unexpected session report contents");
        }
        Files.write(log.toPath(), "ab\\ncd\\nef".getBytes(StandardCharsets.UTF_8));
        try (java.io.RandomAccessFile source = new java.io.RandomAccessFile(log, "r")) {
            // A cut after a newline keeps its line; a cut inside the last line keeps the rest of it.
            long[][] cuts = {{0, 0}, {1, 3}, {3, 3}, {4, 6}, {6, 6}, {7, 7}};
            for (long[] cut : cuts) {
                if (CrashReport.wholeLineStart(source, cut[0], source.length(), new byte[2]) != cut[1])
                    throw new AssertionError("cut " + cut[0] + " did not start on a whole line");
            }
        }
        Files.write(log.toPath(), "ab\\ncd\\n".getBytes(StandardCharsets.UTF_8));
        try (java.io.RandomAccessFile source = new java.io.RandomAccessFile(log, "r")) {
            if (CrashReport.wholeLineStart(source, 4, source.length(), new byte[2]) != 4)
                throw new AssertionError("a cut inside the only remaining line dropped it");
        }
        try (java.io.RandomAccessFile source = new java.io.RandomAccessFile(log, "r")) {
            long end = source.length();
            Files.write(log.toPath(), "late record\\n".getBytes(StandardCharsets.UTF_8),
                    java.nio.file.StandardOpenOption.APPEND);
            ByteArrayOutputStream snapshot = new ByteArrayOutputStream();
            CrashReport.copySnapshot(source, end, snapshot, new byte[3]);
            if (!"ab\\ncd\\n".equals(new String(snapshot.toByteArray(), StandardCharsets.UTF_8)))
                throw new AssertionError("records appended while saving entered the attachment");
        }
        byte[] longLog = new byte[(int) CrashReport.ATTACHMENT_BYTES + 100];
        Arrays.fill(longLog, (byte) 'x');
        longLog[150] = '\\n';
        Files.write(log.toPath(), longLog);
        ByteArrayOutputStream cut = new ByteArrayOutputStream();
        CrashReport.write(cut, "details", null, false, log);
        try (ZipInputStream zip = new ZipInputStream(new ByteArrayInputStream(cut.toByteArray()))) {
            zip.getNextEntry();
            zip.getNextEntry();
            zip.getNextEntry();
            if (read(zip).length != longLog.length - 151)
                throw new AssertionError("a cut client log did not start on a whole line");
        }
        try {
            CrashReport.write(new ByteArrayOutputStream(), "bad trace", new InputStream() {
                public int read() throws IOException { throw new IOException("fixture failure"); }
            }, true);
            throw new AssertionError("unreadable trace produced a successful report");
        } catch (IOException expected) { }
    }
    /** Reads one ZIP entry without depending on post-Java-8 convenience APIs. */
    private static byte[] read(InputStream source) throws IOException {
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        byte[] buffer = new byte[8192];
        for (int count; (count = source.read(buffer)) != -1; ) output.write(buffer, 0, count);
        return output.toByteArray();
    }
}
''')
            classes, android_jar = self.compile_activities(output)
            result = subprocess.run([
                "java", "-classpath", os.pathsep.join((str(classes), str(android_jar))),
                PINS["application_id"] + ".DiagnosticsFixture", str(output / "client.log"),
            ], text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
