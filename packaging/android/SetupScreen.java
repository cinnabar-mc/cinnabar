package @APPLICATION_ID@;

import android.content.ActivityNotFoundException;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.StateListDrawable;
import android.net.Uri;
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
import java.util.function.IntConsumer;
import org.json.JSONArray;
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
    private final Runnable onCancel;
    private ScrollView prompt;

    /** One prompt button; a dismissing one closes the prompt before its action runs. */
    private static final class Choice {
        final String label;
        final boolean dismisses;
        final Runnable action;

        Choice(String label, boolean dismisses, Runnable action) {
            this.label = label;
            this.dismisses = dismisses;
            this.action = action;
        }
    }

    SetupScreen(Context context, Runnable onCancel) {
        super(context);
        this.onCancel = onCancel;
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
        show(title, body, new Choice[] {
            new Choice(acceptLabel, true, accept), new Choice(declineLabel, true, decline)});
    }

    /**
     * Shows one ownership gate screen from native setup's JSON. Buttons open their link in place,
     * quit setup, or dismiss the screen and pass their index to {@code answer}, which is null while
     * a check runs. An unreadable screen answers -1.
     */
    void gate(String message, IntConsumer answer) {
        try {
            JSONObject value = new JSONObject(message);
            JSONArray buttons = value.getJSONArray("buttons");
            Choice[] choices = new Choice[buttons.length()];
            for (int index = 0; index < choices.length; index++) {
                JSONObject button = buttons.getJSONObject(index);
                String link = button.optString("open", null);
                String app = button.optString("app", null);
                int chosen = index;
                if (link != null) {
                    choices[index] = new Choice(button.getString("label"), false, () -> open(link, app));
                } else if (button.optBoolean("quit")) {
                    choices[index] = new Choice(button.getString("label"), false, onCancel);
                } else {
                    choices[index] = new Choice(button.getString("label"), answer != null,
                            () -> { if (answer != null) answer.accept(chosen); });
                }
            }
            // Native paragraphs end at single line breaks; a blank line separates them here.
            show(value.getString("title"), value.getString("body").replace("\n", "\n\n"), choices);
        } catch (JSONException error) {
            android.util.Log.w("SetupScreen", "Cannot read gate screen", error);
            if (answer != null) answer.accept(-1);
        }
    }

    /** Opens {@code link} in {@code app} when given and able, otherwise in whichever app handles it. */
    private void open(String link, String app) {
        Intent intent = new Intent(Intent.ACTION_VIEW, Uri.parse(link));
        if (app != null) {
            try {
                getContext().startActivity(new Intent(intent).setPackage(app));
                return;
            } catch (ActivityNotFoundException error) {
                // Any app that handles the link is the fallback.
            }
        }
        try {
            getContext().startActivity(intent);
        } catch (ActivityNotFoundException error) {
            android.util.Log.w("SetupScreen", "No app opens " + link, error);
        }
    }

    /** Shows a prompt in place of the progress display; the first choice is highlighted. */
    private void show(String title, String body, Choice[] choices) {
        dismissPrompt();
        Context context = getContext();
        cancel.setVisibility(GONE);
        startup.setVisibility(GONE);
        // Limits line length on wide landscape screens.
        LinearLayout column = new LinearLayout(context) {
            @Override protected void onMeasure(int width, int height) {
                int limit = Math.min(MeasureSpec.getSize(width), dp(PROMPT_MAX_WIDTH_DP));
                super.onMeasure(MeasureSpec.makeMeasureSpec(limit, MeasureSpec.EXACTLY), height);
            }
        };
        column.setOrientation(LinearLayout.VERTICAL);
        column.setGravity(Gravity.CENTER_HORIZONTAL);
        int margin = dp(BUTTON_GAP_DP);
        column.setPadding(margin, margin, margin, margin);

        Bitmap logo = startup.logo();
        if (logo != null) {
            ImageView image = new ImageView(context);
            image.setImageBitmap(logo);
            image.setAdjustViewBounds(true);
            image.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
            column.addView(image, new LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.WRAP_CONTENT, dp(LOGO_HEIGHT_DP)));
        }
        TextView heading = text(title, TITLE_SP);
        heading.setGravity(Gravity.CENTER);
        LinearLayout.LayoutParams headingPosition = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        headingPosition.topMargin = margin;
        column.addView(heading, headingPosition);

        TextView message = text("", BODY_SP);
        message.setTextColor(startup.color("foreground") & 0x00ffffff | BODY_ALPHA << 24);
        message.setLinkTextColor(startup.color("accent"));
        message.setLineSpacing(0, BODY_LINE_SPACING);
        message.setAutoLinkMask(Linkify.WEB_URLS);
        message.setText(body);
        // Takes the column's spare height, so the buttons sit at the bottom when everything fits.
        LinearLayout.LayoutParams messagePosition = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT, 1);
        messagePosition.topMargin = margin;
        messagePosition.bottomMargin = margin;
        column.addView(message, messagePosition);

        // A row while the buttons fit, otherwise a column, so large font scales keep every action.
        LinearLayout actions = new LinearLayout(context) {
            @Override protected void onMeasure(int width, int height) {
                arrange(this, HORIZONTAL);
                super.onMeasure(MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED), height);
                if (getMeasuredWidth() > MeasureSpec.getSize(width)) arrange(this, VERTICAL);
                super.onMeasure(width, height);
            }
        };
        actions.setGravity(Gravity.CENTER);
        boolean first = true;
        for (Choice choice : choices) {
            Button view = button(choice.label, first, () -> {
                if (choice.dismisses) dismissPrompt();
                choice.action.run();
            });
            first = false;
            // Grows past the usual height when a large font scale wraps the label.
            view.setMinHeight(dp(BUTTON_HEIGHT_DP));
            actions.addView(view, new LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        }
        column.addView(actions, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        // The whole prompt scrolls when stacked buttons or a large font scale outgrow the screen.
        prompt = new ScrollView(context);
        prompt.setFillViewport(true);
        prompt.setScrollbarFadingEnabled(false);
        prompt.addView(column, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT,
                Gravity.CENTER_HORIZONTAL));
        overlay.addView(prompt, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
        // The prompt takes focus itself, so it opens at the top and no off-screen button is focused;
        // keys scroll it and move on to the buttons.
        prompt.setFocusable(true);
        prompt.setDescendantFocusability(FOCUS_BEFORE_DESCENDANTS);
        prompt.requestFocus();
        announceForAccessibility(title);
    }

    /** Lays out {@code actions} in {@code orientation}, spacing every button after the first. */
    private void arrange(LinearLayout actions, int orientation) {
        actions.setOrientation(orientation);
        for (int index = 1; index < actions.getChildCount(); index++) {
            LinearLayout.LayoutParams position =
                    (LinearLayout.LayoutParams) actions.getChildAt(index).getLayoutParams();
            position.leftMargin = orientation == LinearLayout.HORIZONTAL ? dp(BUTTON_GAP_DP) : 0;
            position.topMargin = orientation == LinearLayout.VERTICAL ? dp(BUTTON_GAP_DP) : 0;
        }
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
