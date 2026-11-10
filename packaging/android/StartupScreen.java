package @APPLICATION_ID@;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.Typeface;
import android.os.SystemClock;
import android.text.Layout;
import android.text.StaticLayout;
import android.text.TextPaint;
import android.view.View;
import android.view.accessibility.AccessibilityNodeInfo;
import android.widget.ProgressBar;
import java.io.InputStream;
import java.io.ByteArrayOutputStream;
import org.json.JSONArray;
import org.json.JSONObject;

/** Original feedback drawn before native resources and the destination interface are ready. */
final class StartupScreen extends View {
    /** A setup fraction for work with no measurable extent, which keeps the animated dots. */
    static final float INDETERMINATE = -1;
    private static final float DETAIL_SIZE = 0.8f;
    private static final int DETAIL_ALPHA = 180;
    /** The opacity of unlit indicators and quiet outlines. */
    static final int UNLIT_ALPHA = 70;
    /** Keeps screen readers from reading every detail update; a new heading is read at once. */
    private static final long ANNOUNCE_INTERVAL_MILLIS = 10_000;

    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.FILTER_BITMAP_FLAG);
    private final TextPaint titlePaint = new TextPaint(Paint.ANTI_ALIAS_FLAG);
    private final TextPaint detailPaint = new TextPaint(Paint.ANTI_ALIAS_FLAG);
    private final JSONObject specification;
    private final Bitmap logo;
    private final JSONObject layout;
    private String stage = "loading_assets";
    private boolean retired;
    private String setupTitle;
    private String setupDetail = "";
    private float setupFraction = INDETERMINATE;
    /** Wrapped setup text, rebuilt only when the text or the view's size changes. */
    private Layout titleLayout;
    private Layout detailLayout;
    private long announcedAt;
    /** Publishes the latest status once the announcement interval allows it. */
    private final Runnable announce = this::announceSetup;

    StartupScreen(Context context) {
        super(context);
        try {
            specification = new JSONObject(read(context, "startup-screen.json"));
            layout = specification.getJSONObject("layout");
            try (InputStream input = context.getAssets().open("startup-title.png")) {
                logo = BitmapFactory.decodeStream(input);
            }
            String[] fonts = context.getAssets().list("resources/fonts");
            if (fonts != null && fonts.length == 1) {
                Typeface typeface = Typeface.createFromAsset(context.getAssets(), "resources/fonts/" + fonts[0]);
                paint.setTypeface(typeface);
                titlePaint.setTypeface(typeface);
                detailPaint.setTypeface(typeface);
            }
        } catch (Exception error) {
            throw new IllegalStateException("Cannot load startup artwork", error);
        }
        setBackgroundColor(color("background"));
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
        setContentDescription(specification.optString(stage));
    }

    /** Applies native work labels; readiness permanently removes this activity's overlay. */
    void update(String message) {
        if (retired) return;
        try {
            JSONObject value = new JSONObject(message);
            if ("ready".equals(value.getString("kind"))) {
                retired = true;
                if (getParent() instanceof android.view.ViewGroup) {
                    ((android.view.ViewGroup) getParent()).removeView(this);
                }
            } else {
                String next = value.getString("stage");
                if (specification.has(next)) stage = next;
                if (setupTitle == null) setContentDescription(specification.optString(stage));
                invalidate();
            }
        } catch (Exception error) {
            android.util.Log.w("StartupScreen", "Cannot read startup progress", error);
        }
    }

    /** Replaces the startup stage with setup's own heading, detail and progress. */
    void setup(String title, String detail, float fraction) {
        if (!title.equals(setupTitle)) titleLayout = null;
        if (!detail.equals(setupDetail)) detailLayout = null;
        boolean newHeading = !title.equals(setupTitle);
        setupTitle = title;
        setupDetail = detail;
        setupFraction = fraction;
        removeCallbacks(announce);
        long wait = announcedAt + ANNOUNCE_INTERVAL_MILLIS - SystemClock.uptimeMillis();
        if (newHeading || wait <= 0) {
            announce.run();
        } else {
            postDelayed(announce, wait);
        }
        invalidate();
    }

    /** Publishes the current setup status to accessibility services. */
    private void announceSetup() {
        announcedAt = SystemClock.uptimeMillis();
        setAccessibilityLiveRegion(ACCESSIBILITY_LIVE_REGION_POLITE);
        setContentDescription(setupDetail.isEmpty() ? setupTitle : setupTitle + ", " + setupDetail);
    }

    @Override protected void onDetachedFromWindow() {
        removeCallbacks(announce);
        super.onDetachedFromWindow();
    }

    /** Returns to the startup stage's label and dots. */
    void clearSetup() {
        removeCallbacks(announce);
        setupTitle = null;
        setupDetail = "";
        setupFraction = INDETERMINATE;
        titleLayout = null;
        detailLayout = null;
        setAccessibilityLiveRegion(ACCESSIBILITY_LIVE_REGION_NONE);
        setContentDescription(specification.optString(stage));
        invalidate();
    }

    /** Reports measured setup progress to accessibility services as a progress bar. */
    @Override public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info) {
        super.onInitializeAccessibilityNodeInfo(info);
        if (setupFraction == INDETERMINATE) return;
        info.setClassName(ProgressBar.class.getName());
        info.setRangeInfo(AccessibilityNodeInfo.RangeInfo.obtain(
                AccessibilityNodeInfo.RangeInfo.RANGE_TYPE_PERCENT, 0, 100, setupFraction * 100));
    }

    /** The bundled title artwork, for screens drawn alongside this one. */
    Bitmap logo() { return logo; }

    /** The bundled font, or null when the platform default applies. */
    Typeface typeface() { return paint.getTypeface(); }

    @Override protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
        titleLayout = null;
        detailLayout = null;
    }

    /** Reads one bundled specification without depending on the unpacked prep kit. */
    private static String read(Context context, String path) throws Exception {
        try (InputStream input = context.getAssets().open(path);
                ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] buffer = new byte[4096];
            for (int count; (count = input.read(buffer)) != -1;) output.write(buffer, 0, count);
            return new String(output.toByteArray(), java.nio.charset.StandardCharsets.UTF_8);
        }
    }

    /** Converts the shared RGBA color to Android's ARGB representation. */
    int color(String key) {
        JSONArray rgba = specification.optJSONArray(key);
        return android.graphics.Color.argb(rgba.optInt(3), rgba.optInt(0), rgba.optInt(1), rgba.optInt(2));
    }

    /** Reads one shared design-space metric. */
    private float metric(String name) { return (float) layout.optDouble(name); }

    @Override protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        float width = getWidth(), height = getHeight();
        float scale = Math.max(metric("minimum_scale"), Math.min(width / (float) layout.optJSONArray("design_size").optDouble(0),
                height / (float) layout.optJSONArray("design_size").optDouble(1)));
        if (logo != null) {
            float w = Math.min(metric("logo_width") * scale, width - metric("side_margin") * scale);
            float h = w * logo.getHeight() / logo.getWidth();
            paint.setAlpha(255);
            canvas.drawBitmap(logo, null, new android.graphics.RectF((width-w)/2, height*metric("logo_center_y")-h/2,
                    (width+w)/2, height*metric("logo_center_y")+h/2), paint);
        }
        float indicatorTop = height*metric("indicator_top");
        if (setupTitle == null) {
            paint.setColor(color("foreground"));
            paint.setTextSize(metric("label_size") * scale);
            paint.setTextAlign(Paint.Align.CENTER);
            canvas.drawText(specification.optString(stage), width/2, height*metric("label_top")-paint.ascent(), paint);
        } else {
            indicatorTop = drawSetup(canvas, scale, indicatorTop);
        }
        if (setupFraction == INDETERMINATE) {
            int active = (int) ((SystemClock.uptimeMillis()/layout.optLong("step_millis")) % 3);
            for (int i=0; i<3; ++i) {
                paint.setColor(color(i == active ? "accent" : "foreground"));
                paint.setAlpha(i == active ? 255 : UNLIT_ALPHA);
                float x = width/2 + (i-1)*metric("dot_spacing")*scale - metric("dot_size")*.5f*scale;
                canvas.drawRect(x, indicatorTop, x+metric("dot_size")*scale, indicatorTop+metric("dot_size")*scale, paint);
            }
            paint.setAlpha(255);
            if (!retired && isShown()) postInvalidateDelayed(layout.optLong("frame_millis"));
        }
    }

    /** Draws setup's heading, bar and detail, and returns where an animated indicator belongs. */
    private float drawSetup(Canvas canvas, float scale, float indicatorTop) {
        float width = getWidth(), height = getHeight();
        float margin = metric("side_margin") * scale;
        int textWidth = Math.max(1, (int) (width - 2*margin));
        if (titleLayout == null) {
            titlePaint.setTextSize(metric("label_size") * scale);
            titleLayout = centered(setupTitle, titlePaint, textWidth);
        }
        if (detailLayout == null) {
            detailPaint.setTextSize(metric("label_size") * DETAIL_SIZE * scale);
            detailLayout = centered(setupDetail, detailPaint, textWidth);
        }
        float titleTop = height*metric("label_top");
        titlePaint.setColor(color("foreground"));
        drawAt(canvas, titleLayout, margin, titleTop);
        float thickness = metric("dot_size") * scale;
        float barTop = Math.max(indicatorTop, titleTop + titleLayout.getHeight() + thickness);
        if (setupFraction != INDETERMINATE) {
            float barWidth = Math.min(metric("logo_width") * scale, width - 2*margin);
            float left = (width - barWidth) / 2;
            paint.setColor(color("foreground"));
            paint.setAlpha(UNLIT_ALPHA);
            canvas.drawRect(left, barTop, left + barWidth, barTop + thickness, paint);
            paint.setColor(color("accent"));
            canvas.drawRect(left, barTop, left + barWidth*Math.min(1, setupFraction), barTop + thickness, paint);
            paint.setAlpha(255);
        }
        detailPaint.setColor(color("foreground"));
        detailPaint.setAlpha(DETAIL_ALPHA);
        drawAt(canvas, detailLayout, margin, barTop + thickness*2);
        return barTop;
    }

    private static Layout centered(String text, TextPaint paint, int width) {
        return StaticLayout.Builder.obtain(text, 0, text.length(), paint, width)
                .setAlignment(Layout.Alignment.ALIGN_CENTER)
                .setBreakStrategy(Layout.BREAK_STRATEGY_BALANCED).build();
    }

    private static void drawAt(Canvas canvas, Layout text, float x, float y) {
        canvas.save();
        canvas.translate(x, y);
        text.draw(canvas);
        canvas.restore();
    }
}
