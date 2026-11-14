package @APPLICATION_ID@;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;

/**
 * Keeps the game's process important while the player approves a Microsoft sign-in code in the
 * browser. Android treats a hidden game as a cached process, pauses it and ends it first when
 * the browser needs memory; a foreground service keeps it running, so the sign-in helper sees
 * the approval and saves the account even before the player returns. The hold lasts at most
 * {@link #LIMIT_MS} per trip to the browser and ends as soon as sign-in settles. All state is
 * touched on the activity thread only.
 */
public final class SignInHoldService extends Service {
    private static final String CHANNEL = "sign_in";
    private static final int NOTIFICATION = 0x5349;
    /** Android 14 and later stop a short service after about this long; earlier ones do not. */
    private static final long LIMIT_MS = 3 * 60 * 1000;

    private static boolean wanted;
    private static SignInHoldService running;

    private final Handler handler = new Handler(Looper.getMainLooper());
    private final Runnable expire = this::finish;

    /**
     * Starts or renews the hold. Android only allows the start while the game is visible; a
     * refused start leaves sign-in to resume when the player returns.
     */
    static void hold(Context context) {
        wanted = true;
        try {
            context.startService(new Intent(context, SignInHoldService.class));
        } catch (RuntimeException refused) {
            wanted = false;
        }
    }

    /** Ends the hold once sign-in finished, failed or was cancelled. */
    static void release() {
        wanted = false;
        if (running != null) running.finish();
    }

    @Override public void onCreate() {
        super.onCreate();
        running = this;
    }

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        handler.removeCallbacks(expire);
        if (!wanted) {
            finish();
            return START_NOT_STICKY;
        }
        try {
            if (Build.VERSION.SDK_INT >= 34) {
                startForeground(NOTIFICATION, notification(),
                        ServiceInfo.FOREGROUND_SERVICE_TYPE_SHORT_SERVICE);
            } else {
                startForeground(NOTIFICATION, notification());
            }
        } catch (RuntimeException refused) {
            finish();
            return START_NOT_STICKY;
        }
        handler.postDelayed(expire, LIMIT_MS);
        return START_NOT_STICKY;
    }

    /** Android 14 and later report the short service's time limit here. */
    @Override public void onTimeout(int startId) {
        finish();
    }

    @Override public void onDestroy() {
        handler.removeCallbacks(expire);
        if (running == this) running = null;
        super.onDestroy();
    }

    @Override public IBinder onBind(Intent intent) {
        return null;
    }

    private void finish() {
        handler.removeCallbacks(expire);
        stopForeground(STOP_FOREGROUND_REMOVE);
        stopSelf();
    }

    /** A silent notice that returns to the game when tapped, shown where notifications are allowed. */
    private Notification notification() {
        NotificationManager manager =
                (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
        NotificationChannel channel =
                new NotificationChannel(CHANNEL, "Sign-in", NotificationManager.IMPORTANCE_LOW);
        channel.setShowBadge(false);
        manager.createNotificationChannel(channel);
        Notification.Builder builder = new Notification.Builder(this, CHANNEL)
                .setSmallIcon(android.R.drawable.stat_notify_sync_noanim)
                .setContentTitle("Finishing Microsoft sign-in")
                .setContentText("Return to @PRODUCT_NAME@ after approving the code in your browser.")
                .setOngoing(true);
        Intent game = getPackageManager().getLaunchIntentForPackage(getPackageName());
        if (game != null) {
            builder.setContentIntent(PendingIntent.getActivity(this, 0, game,
                    PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT));
        }
        return builder.build();
    }
}
