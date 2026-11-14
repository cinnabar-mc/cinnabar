package auth

import (
	"context"
	"errors"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"golang.org/x/oauth2"
)

// fakeApp stands in for the Switch app: its device-code sign-in finishes when finish is
// closed, and its identity token names how many times it was asked for.
type fakeApp struct {
	finish chan struct{}

	mu         sync.Mutex
	deviceAuth int
	identity   int
}

func newFakeApp() *fakeApp { return &fakeApp{finish: make(chan struct{})} }

func (a *fakeApp) DeviceAuth(context.Context) (*oauth2.DeviceAuthResponse, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.deviceAuth++
	return &oauth2.DeviceAuthResponse{
		DeviceCode: "device", UserCode: "TESTCODE", VerificationURI: "https://www.microsoft.com/link",
		Expiry: time.Now().Add(15 * time.Minute),
	}, nil
}

func (a *fakeApp) DeviceAccessToken(ctx context.Context, _ *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
	select {
	case <-a.finish:
		return &oauth2.Token{AccessToken: "switch-access", RefreshToken: "switch-refresh", Expiry: time.Now().Add(time.Hour)}, nil
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

func (a *fakeApp) TokenSource(_ context.Context, token *oauth2.Token) oauth2.TokenSource {
	return oauth2.StaticTokenSource(token)
}

func (a *fakeApp) IdentityToken(_ context.Context, source oauth2.TokenSource) (string, error) {
	if _, err := source.Token(); err != nil {
		return "", err
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	a.identity++
	return "XBL3.0 x=hash;switch", nil
}

func (a *fakeApp) counts() (deviceAuth, identity int) {
	a.mu.Lock()
	defer a.mu.Unlock()
	return a.deviceAuth, a.identity
}

func newTestKeychain(t *testing.T, app MicrosoftApp, services Services) *Keychain {
	t.Helper()
	keychain := NewKeychain(KeychainConfig{
		Path:     filepath.Join(t.TempDir(), CacheFile),
		App:      app,
		Services: services,
	})
	t.Cleanup(keychain.Close)
	return keychain
}

// Without a sign-in, a join gets the code to enter; once the player enters it the token is
// saved and the account signs in as its Java profile.
func TestKeychainSignsInOnceThePlayerEntersTheCode(t *testing.T) {
	fake := newFakeServices(t)
	app := newFakeApp()
	keychain := newTestKeychain(t, app, fake.services())
	ctx := context.Background()

	if keychain.SignedIn() {
		t.Fatal("a new keychain is signed in")
	}
	if _, err := keychain.Session(ctx); !errors.Is(err, ErrNotSignedIn) {
		t.Fatalf("session without a sign-in = %v", err)
	}
	prompt, err := keychain.BeginSignIn(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if prompt.UserCode != "TESTCODE" || prompt.VerificationURI != "https://www.microsoft.com/link" {
		t.Fatalf("prompt = %+v", prompt)
	}
	// A second join while the player has not finished shows the same code.
	again, err := keychain.BeginSignIn(ctx)
	if err != nil || *again != *prompt {
		t.Fatalf("second prompt = %+v, %v", again, err)
	}
	if started, _ := app.counts(); started != 1 {
		t.Fatalf("%d device-code sign-ins started", started)
	}

	close(app.finish)
	testwait.Eventually(t, 5*time.Second, "the sign-in to finish", keychain.SignedIn)
	if _, err := os.Stat(keychain.config.Path); err != nil {
		t.Fatalf("the token was not saved: %v", err)
	}
	session, err := keychain.Session(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if session.Profile.Name != "Notch" || fake.identityToken != "XBL3.0 x=hash;switch" {
		t.Fatalf("session %+v from identity %q", session, fake.identityToken)
	}
}

// The services limit how often an account signs in, so a session is reused until shortly
// before it expires.
func TestKeychainReusesASessionUntilItExpires(t *testing.T) {
	fake := newFakeServices(t)
	app := newFakeApp()
	now := time.Now()
	keychain := NewKeychain(KeychainConfig{
		Path:     filepath.Join(t.TempDir(), CacheFile),
		App:      app,
		Services: fake.services(),
		Now:      func() time.Time { return now },
	})
	t.Cleanup(keychain.Close)
	if _, err := keychain.BeginSignIn(context.Background()); err != nil {
		t.Fatal(err)
	}
	close(app.finish)
	testwait.Eventually(t, 5*time.Second, "the sign-in to finish", keychain.SignedIn)

	for range 3 {
		if _, err := keychain.Session(context.Background()); err != nil {
			t.Fatal(err)
		}
	}
	if _, identity := app.counts(); identity != 1 {
		t.Fatalf("signed in %d times for three joins", identity)
	}
	// The fake session lasts a day; close to its end the keychain signs in again.
	now = now.Add(24*time.Hour - time.Minute)
	if _, err := keychain.Session(context.Background()); err != nil {
		t.Fatal(err)
	}
	if _, identity := app.counts(); identity != 2 {
		t.Fatalf("signed in %d times after the session neared its expiry", identity)
	}
}

// A join the session service refuses drops the session, so the next join signs in again.
func TestKeychainDropsARefusedSession(t *testing.T) {
	fake := newFakeServices(t)
	app := newFakeApp()
	keychain := newTestKeychain(t, app, fake.services())
	if _, err := keychain.BeginSignIn(context.Background()); err != nil {
		t.Fatal(err)
	}
	close(app.finish)
	testwait.Eventually(t, 5*time.Second, "the sign-in to finish", keychain.SignedIn)
	session, err := keychain.Session(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	fake.joinStatus = http.StatusForbidden
	if err := keychain.Join(context.Background(), session, "hash"); err == nil {
		t.Fatal("a refused join succeeded")
	}
	if _, err := keychain.Session(context.Background()); err != nil {
		t.Fatal(err)
	}
	if _, identity := app.counts(); identity != 2 {
		t.Fatalf("signed in %d times; the refused session was reused", identity)
	}
}

// Signing out of the account forgets the Java Edition sign-in too.
func TestKeychainForgetRemovesTheSignIn(t *testing.T) {
	fake := newFakeServices(t)
	app := newFakeApp()
	keychain := newTestKeychain(t, app, fake.services())
	if _, err := keychain.BeginSignIn(context.Background()); err != nil {
		t.Fatal(err)
	}
	close(app.finish)
	testwait.Eventually(t, 5*time.Second, "the sign-in to finish", keychain.SignedIn)
	if err := keychain.Forget(context.Background()); err != nil {
		t.Fatal(err)
	}
	if keychain.SignedIn() {
		t.Fatal("still signed in after Forget")
	}
	if _, err := keychain.Session(context.Background()); !errors.Is(err, ErrNotSignedIn) {
		t.Fatalf("session after Forget = %v", err)
	}
}

func TestCachePathSitsBesideTheAccountToken(t *testing.T) {
	if got := CachePath(filepath.Join("auth", "microsoft-token.json")); got != filepath.Join("auth", CacheFile) {
		t.Fatalf("CachePath = %q", got)
	}
}
