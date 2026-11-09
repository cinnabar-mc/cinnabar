package authflow

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"golang.org/x/oauth2"
)

// savedPromptFlow issues one device code for the next Run's DeviceAuth.
func savedPromptFlow(code string, expiry time.Time) func(context.Context) (*oauth2.DeviceAuthResponse, error) {
	return func(context.Context) (*oauth2.DeviceAuthResponse, error) {
		return &oauth2.DeviceAuthResponse{
			DeviceCode: "device-" + code, UserCode: code,
			VerificationURI: "https://login.example.test/device", Expiry: expiry, Interval: 5,
		}, nil
	}
}

func TestDeviceSignInResumesTheSameCodeAfterTheProcessEnds(t *testing.T) {
	path := filepath.Join(t.TempDir(), "auth", "token.json")
	ctx, end := context.WithCancel(context.Background())
	var first bytes.Buffer
	err := Run(ctx, Config{
		Path: path, Writer: &first, Refresh: staticRefresh,
		DeviceAuth: savedPromptFlow("ABCD-1234", time.Now().Add(15*time.Minute)),
		DeviceToken: func(ctx context.Context, _ *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
			// The phone ends the client while the player approves the code in the browser.
			end()
			return nil, ctx.Err()
		},
	})
	if err == nil {
		t.Fatal("the ended sign-in succeeded")
	}
	if _, err := os.Stat(authcache.PendingPromptPath(path)); err != nil {
		t.Fatalf("the ended sign-in did not keep its prompt: %v", err)
	}

	var second bytes.Buffer
	err = Run(context.Background(), Config{
		Path: path, Writer: &second, Refresh: staticRefresh,
		DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			t.Fatal("the next launch requested a new code instead of resuming the approved one")
			return nil, errors.New("unreachable")
		},
		DeviceToken: func(_ context.Context, response *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
			if response.DeviceCode != "device-ABCD-1234" || response.Interval != 5 {
				t.Fatalf("resumed prompt = %q every %ds, want the saved one", response.DeviceCode, response.Interval)
			}
			return validToken("access", "refresh"), nil
		},
	})
	if err != nil {
		t.Fatalf("resumed sign-in failed: %v", err)
	}
	events := decodeEvents(t, second.Bytes())
	if len(events) != 3 || events[1].Kind != "device_code" || events[1].UserCode != "ABCD-1234" ||
		events[2].Kind != "authenticated" {
		t.Fatalf("resumed sign-in events = %#v, want the saved code then authenticated", events)
	}
	if _, err := os.Stat(authcache.PendingPromptPath(path)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("the completed sign-in kept its prompt: %v", err)
	}
}

func TestDeviceSignInForgetsSettledAndExpiredCodes(t *testing.T) {
	path := filepath.Join(t.TempDir(), "token.json")
	err := Run(context.Background(), Config{
		Path: path, Writer: &bytes.Buffer{}, Refresh: staticRefresh,
		DeviceAuth: savedPromptFlow("DENY-1234", time.Now().Add(15*time.Minute)),
		DeviceToken: func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
			return nil, &oauth2.RetrieveError{ErrorCode: "access_denied"}
		},
	})
	if err == nil {
		t.Fatal("the denied sign-in succeeded")
	}
	if _, err := os.Stat(authcache.PendingPromptPath(path)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("the denied code was kept for resuming: %v", err)
	}

	if err := authcache.SavePendingPrompt(path, &oauth2.DeviceAuthResponse{
		DeviceCode: "device-OLD", UserCode: "OLD-CODE", VerificationURI: "https://login.example.test/device",
		Expiry: time.Now().Add(-time.Minute),
	}); err != nil {
		t.Fatal(err)
	}
	var output bytes.Buffer
	err = Run(context.Background(), Config{
		Path: path, Writer: &output, Refresh: staticRefresh,
		DeviceAuth: savedPromptFlow("NEW-CODE", time.Now().Add(15*time.Minute)),
		DeviceToken: func(_ context.Context, response *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
			if response.UserCode != "NEW-CODE" {
				t.Fatalf("polled %q, want a fresh code after the saved one expired", response.UserCode)
			}
			return validToken("access", "refresh"), nil
		},
	})
	if err != nil {
		t.Fatalf("sign-in after an expired saved code failed: %v", err)
	}
	if events := decodeEvents(t, output.Bytes()); events[1].UserCode != "NEW-CODE" {
		t.Fatalf("prompt code = %q, want the fresh one", events[1].UserCode)
	}
}

func TestPendingPromptIsPrivateAndCarriesAnAbsoluteExpiry(t *testing.T) {
	path := filepath.Join(t.TempDir(), "token.json")
	expiry := time.Unix(time.Now().Add(10*time.Minute).Unix(), 0)
	if err := authcache.SavePendingPrompt(path, &oauth2.DeviceAuthResponse{
		DeviceCode: "device", UserCode: "ABCD-1234", VerificationURI: "https://login.example.test/device", Expiry: expiry,
	}); err != nil {
		t.Fatal(err)
	}
	if resumed := authcache.LoadPendingPrompt(path, expiry.Add(-time.Second)); resumed == nil || !resumed.Expiry.Equal(expiry) {
		t.Fatalf("prompt before expiry = %#v, want it with its saved expiry", resumed)
	}
	if resumed := authcache.LoadPendingPrompt(path, expiry); resumed != nil {
		t.Fatal("an expired prompt was resumed")
	}
	if _, err := os.Stat(authcache.PendingPromptPath(path)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("the expired prompt was kept: %v", err)
	}
}

func TestDeviceSignInForgetsACodeRedeemedAsTheProcessEnds(t *testing.T) {
	path := filepath.Join(t.TempDir(), "token.json")
	ctx, end := context.WithCancel(context.Background())
	defer end()
	_, _ = DeviceFlow{
		Pending:   path,
		Authorize: savedPromptFlow("LAST-1234", time.Now().Add(15*time.Minute)),
		Token: func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
			// The token arrives just as the client's process ends.
			end()
			return validToken("access", "refresh"), nil
		},
	}.Request(ctx, func(*oauth2.DeviceAuthResponse) error { return nil })
	if _, err := os.Stat(authcache.PendingPromptPath(path)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("a redeemed code was kept for resuming: %v", err)
	}
}
