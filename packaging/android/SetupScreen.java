package @APPLICATION_ID@;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.StateListDrawable;
import android.os.Build;
import android.text.util.Linkify;
import android.util.TypedValue;
import android.view.DisplayCutout;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.FrameLayout;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import org.json.JSONException;
import org.json.JSONObject;

/** The setup activity's interface: the startup screen, its cancel control and its prompts. */
final class SetupScreen extends FrameLayout {
    private static final int PROMPT_MAX_WIDTH_DP = 560;
    private static final int BUTTON_HEIGHT_DP = 48;
    private static final int BUTTON_GAP_DP = 12;
    private static final int LOGO_HEIGHT_DP = 48;
    private static final int TITLE_SP = 20;
    private static final int BODY_SP = 14;
    private static final int BODY_ALPHA = 220;
    /** The bundled font's own line gap leaves link underlines touching the next line. */
    private static final float BODY_LINE_SPACING = 1.3f;
    private static final int BUTTON_SP = 15;

    private final StartupScreen startup;
    private final FrameLayout overlay;
    private final Button cancel;
    private LinearLayout prompt;

    SetupScreen(Context context, Runnable onCancel) {
        super(context);
        startup = new StartupScreen(context);
        // The window background holds the launcher icon, which must not show behind a prompt.
        setBackgroundColor(startup.color("background"));
        addView(startup, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
        overlay = new FrameLayout(context);
        addView(overlay, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
        cancel = button("", false, onCancel);
        cancel.setVisibility(GONE);
        // The corner stays clear of the centered startup column on every landscape screen.
        LayoutParams position = new LayoutParams(LayoutParams.WRAP_CONTENT, dp(BUTTON_HEIGHT_DP),
                Gravity.BOTTOM | Gravity.END);
        position.bottomMargin = dp(BUTTON_GAP_DP);
        position.setMarginEnd(dp(BUTTON_GAP_DP));
        overlay.addView(cancel, position);
        setOnApplyWindowInsetsListener((view, insets) -> {
            int left = insets.getSystemWindowInsetLeft(), top = insets.getSystemWindowInsetTop();
            int right = insets.getSystemWindowInsetRight(), bottom = insets.getSystemWindowInsetBottom();
            if (Build.VERSION.SDK_INT >= 28 && insets.getDisplayCutout() != null) {
                DisplayCutout cutout = insets.getDisplayCutout();
                left = Math.max(left, cutout.getSafeInsetLeft());
                top = Math.max(top, cutout.getSafeInsetTop());
                right = Math.max(right, cutout.getSafeInsetRight());
                bottom = Math.max(bottom, cutout.getSafeInsetBottom());
            }
            // A cutout on one side must not pull prompts off the startup screen's center line.
            int side = Math.max(left, right);
            overlay.setPadding(side, top, side, bottom);
            return insets;
        });
    }

    /** Applies one JSON progress update from native setup, dismissing any prompt. */
    void progress(String message) {
        dismissPrompt();
        try {
            JSONObject value = new JSONObject(message);
            float fraction = value.isNull("fraction") ? StartupScreen.INDETERMINATE
                    : (float) value.getDouble("fraction");
            startup.setup(value.getString("title"), value.getString("detail"), fraction);
            if (value.isNull("cancel")) {
                cancel.setVisibility(GONE);
            } else {
                cancel.setText(value.getString("cancel"));
                cancel.setVisibility(VISIBLE);
            }
        } catch (JSONException error) {
            android.util.Log.w("SetupScreen", "Cannot read setup progress", error);
        }
    }

    /** Returns to plain startup feedback while setup has reported nothing. */
    void restart() {
        dismissPrompt();
        cancel.setVisibility(GONE);
        startup.clearSetup();
    }

    /** Asks a question in place of the progress display; either answer dismisses the prompt. */
    void prompt(String title, String body, String acceptLabel, Runnable accept,
            String declineLabel, Runnable decline) {
        dismissPrompt();
        Context context = getContext();
        cancel.setVisibility(GONE);
        startup.setVisibility(GONE);
        // Limits line length on wide landscape screens.
        prompt = new LinearLayout(context) {
            @Override protected void onMeasure(int width, int height) {
                int limit = Math.min(MeasureSpec.getSize(width), dp(PROMPT_MAX_WIDTH_DP));
                super.onMeasure(MeasureSpec.makeMeasureSpec(limit, MeasureSpec.EXACTLY), height);
            }
        };
        prompt.setOrientation(LinearLayout.VERTICAL);
        prompt.setGravity(Gravity.CENTER_HORIZONTAL);
        int margin = dp(BUTTON_GAP_DP);
        prompt.setPadding(margin, margin, margin, margin);

        Bitmap logo = startup.logo();
        if (logo != null) {
            ImageView image = new ImageView(context);
            image.setImageBitmap(logo);
            image.setAdjustViewBounds(true);
            image.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
            prompt.addView(image, new LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.WRAP_CONTENT, dp(LOGO_HEIGHT_DP)));
        }
        TextView heading = text(title, TITLE_SP);
        heading.setGravity(Gravity.CENTER);
        LinearLayout.LayoutParams headingPosition = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        headingPosition.topMargin = margin;
        prompt.addView(heading, headingPosition);

        TextView message = text("", BODY_SP);
        message.setTextColor(startup.color("foreground") & 0x00ffffff | BODY_ALPHA << 24);
        message.setLinkTextColor(startup.color("accent"));
        message.setLineSpacing(0, BODY_LINE_SPACING);
        message.setAutoLinkMask(Linkify.WEB_URLS);
        message.setText(body);
        ScrollView scroll = new ScrollView(context);
        scroll.setScrollbarFadingEnabled(false);
        scroll.addView(message);
        LinearLayout.LayoutParams scrollPosition = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1);
        scrollPosition.topMargin = margin;
        scrollPosition.bottomMargin = margin;
        prompt.addView(scroll, scrollPosition);

        LinearLayout actions = new LinearLayout(context);
        actions.setGravity(Gravity.CENTER);
        Button primary = button(acceptLabel, true, () -> { dismissPrompt(); accept.run(); });
        actions.addView(primary, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, dp(BUTTON_HEIGHT_DP)));
        LinearLayout.LayoutParams second = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, dp(BUTTON_HEIGHT_DP));
        second.leftMargin = margin;
        actions.addView(button(declineLabel, false, () -> { dismissPrompt(); decline.run(); }), second);
        prompt.addView(actions, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        overlay.addView(prompt, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT,
                Gravity.CENTER_HORIZONTAL));
        primary.requestFocus();
        announceForAccessibility(title);
    }

    private void dismissPrompt() {
        if (prompt != null) overlay.removeView(prompt);
        prompt = null;
        startup.setVisibility(VISIBLE);
    }

    private TextView text(String value, int sizeSp) {
        TextView view = new TextView(getContext());
        view.setText(value);
        view.setTextColor(startup.color("foreground"));
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp);
        view.setTypeface(startup.typeface());
        return view;
    }

    /** Square buttons in the startup palette: a filled accent or a quiet outline. */
    private Button button(String label, boolean filled, Runnable action) {
        Button view = new Button(getContext());
        view.setText(label);
        view.setAllCaps(false);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, BUTTON_SP);
        view.setTypeface(startup.typeface());
        view.setTextColor(startup.color("foreground"));
        view.setStateListAnimator(null);
        view.setPadding(dp(BUTTON_GAP_DP * 2), 0, dp(BUTTON_GAP_DP * 2), 0);
        int foreground = startup.color("foreground");
        int accent = startup.color("accent");
        StateListDrawable states = new StateListDrawable();
        states.addState(new int[] {android.R.attr.state_pressed},
                face(filled ? accent & 0x00ffffff | 0xb0000000 : foreground & 0x00ffffff | StartupScreen.UNLIT_ALPHA << 24,
                        filled ? accent : foreground));
        states.addState(new int[] {android.R.attr.state_focused},
                face(filled ? accent : 0, foreground));
        states.addState(new int[0], face(filled ? accent : 0,
                filled ? accent : foreground & 0x00ffffff | StartupScreen.UNLIT_ALPHA << 24));
        view.setBackground(states);
        view.setOnClickListener(clicked -> action.run());
        return view;
    }

    private Drawable face(int fill, int outline) {
        GradientDrawable face = new GradientDrawable();
        face.setShape(GradientDrawable.RECTANGLE);
        face.setColor(fill);
        face.setStroke(dp(1), outline);
        return face;
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }
}
