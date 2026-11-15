package auth

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"golang.org/x/oauth2"
)

// fakeApp stands in for the Switch app: its device-code sign-in finishes when finish is
// closed, and its identity token names how many times it was asked for.
type fakeApp struct {
	finish chan struct{}
	// revoked makes the identity token fail as Microsoft refusing a revoked token.
	revoked bool
	// stall, when not nil, holds every identity token request until it is closed.
	stall chan struct{}
	// declined makes the device-code sign-in end without a token once finish closes.
	declined bool
	// refreshErr, when not nil, fails every refresh of a saved token.
	refreshErr error

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
		if a.declined {
			return nil, errors.New("authorization_declined")
		}
		return &oauth2.Token{AccessToken: "switch-access", RefreshToken: "switch-refresh", Expiry: time.Now().Add(time.Hour)}, nil
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

func (a *fakeApp) TokenSource(_ context.Context, token *oauth2.Token) oauth2.TokenSource {
	return fakeSource{app: a, token: token}
}

// fakeSource hands out its token, or the app's refresh failure.
type fakeSource struct {
	app   *fakeApp
	token *oauth2.Token
}

func (s fakeSource) Token() (*oauth2.Token, error) {
	s.app.mu.Lock()
	defer s.app.mu.Unlock()
	if s.app.refreshErr != nil {
		return nil, s.app.refreshErr
	}
	return s.token, nil
}

func (a *fakeApp) IdentityToken(ctx context.Context, source oauth2.TokenSource) (string, error) {
	if _, err := source.Token(); err != nil {
		return "", err
	}
	if a.stall != nil {
		// Like a real request, a stalled one ends when its keychain closes.
		select {
		case <-a.stall:
		case <-ctx.Done():
			return "", ctx.Err()
		}
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	a.identity++
	if a.revoked {
		return "", fmt.Errorf("xbox token: %w", &oauth2.RetrieveError{ErrorCode: "invalid_grant"})
	}
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
		Path:     filepath.Join(t.TempDir(), "token.json"),
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
		Path:     filepath.Join(t.TempDir(), "token.json"),
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

// Signing out while a sign-in waits for the player stops it: entering the code afterwards
// must not bring the Java Edition sign-in back.
func TestKeychainForgetStopsAPendingSignIn(t *testing.T) {
	fake := newFakeServices(t)
	app := newFakeApp()
	keychain := newTestKeychain(t, app, fake.services())
	if _, err := keychain.BeginSignIn(context.Background()); err != nil {
		t.Fatal(err)
	}
	if err := keychain.Forget(context.Background()); err != nil {
		t.Fatal(err)
	}
	close(app.finish)
	keychain.work.Wait()
	if keychain.SignedIn() {
		t.Fatal("the forgotten sign-in was saved")
	}
	if _, err := os.Stat(keychain.config.Path); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("token file after Forget: %v", err)
	}
}

// A saved sign-in that can no longer be read or refreshed reads as no sign-in, so the next
// online-mode join asks for a new one instead of failing until the player signs out.
func TestKeychainAnUnusableSavedSignInAsksAgain(t *testing.T) {
	fake := newFakeServices(t)
	keychain := newTestKeychain(t, newFakeApp(), fake.services())
	if err := os.WriteFile(keychain.config.Path, []byte("not a token"), 0o600); err != nil {
		t.Fatal(err)
	}
	if !keychain.SignedIn() {
		t.Fatal("a saved sign-in is not reported")
	}
	if _, err := keychain.Session(context.Background()); !errors.Is(err, ErrNotSignedIn) {
		t.Fatalf("session from an unreadable sign-in = %v", err)
	}
	// Joins no longer try the dead token before dialing.
	if keychain.SignedIn() {
		t.Fatal("an unusable sign-in is still reported as signed in")
	}
}

// A sign-in Microsoft refuses, such as a revoked one, reads as no sign-in too.
func TestKeychainARevokedSignInAsksAgain(t *testing.T) {
	fake := newFakeServices(t)
	app := newFakeApp()
	keychain := newTestKeychain(t, app, fake.services())
	if _, err := keychain.BeginSignIn(context.Background()); err != nil {
		t.Fatal(err)
	}
	close(app.finish)
	testwait.Eventually(t, 5*time.Second, "the sign-in to finish", keychain.SignedIn)
	app.mu.Lock()
	app.revoked = true
	app.mu.Unlock()
	if _, err := keychain.Session(context.Background()); !errors.Is(err, ErrNotSignedIn) {
		t.Fatalf("session from a revoked sign-in = %v", err)
	}
}

// A refresh Microsoft does not answer, from a network failure or a server error, keeps the
// saved sign-in for the next join; only a refusal reads as no sign-in.
func TestKeychainKeepsASignInMicrosoftDidNotRefresh(t *testing.T) {
	cases := []struct {
		name       string
		refreshErr error
		want       error
	}{
		{"network", &url.Error{Op: "Post", URL: "https://login.live.com", Err: errors.New("connection refused")}, ErrServicesUnreachable},
		{"server error", &oauth2.RetrieveError{Response: &http.Response{StatusCode: http.StatusServiceUnavailable}}, ErrServicesUnreachable},
		{"throttled", &oauth2.RetrieveError{Response: &http.Response{StatusCode: http.StatusTooManyRequests}}, ErrServicesUnreachable},
		{"refused", &oauth2.RetrieveError{Response: &http.Response{StatusCode: http.StatusBadRequest}, ErrorCode: "invalid_grant"}, ErrNotSignedIn},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			fake := newFakeServices(t)
			app := newFakeApp()
			signedIn := newTestKeychain(t, app, fake.services())
			if _, err := signedIn.BeginSignIn(context.Background()); err != nil {
				t.Fatal(err)
			}
			close(app.finish)
			testwait.Eventually(t, 5*time.Second, "the sign-in to finish", signedIn.SignedIn)
			signedIn.Close()

			app.mu.Lock()
			app.refreshErr = test.refreshErr
			app.mu.Unlock()
			// A later core opens the saved token and has to refresh it.
			keychain := NewKeychain(KeychainConfig{Path: signedIn.config.Path, App: app, Services: fake.services()})
			t.Cleanup(keychain.Close)
			_, err := keychain.Session(context.Background())
			if !errors.Is(err, test.want) {
				t.Fatalf("session = %v, want %v", err, test.want)
			}
			if test.want == ErrNotSignedIn {
				return
			}
			if !keychain.SignedIn() {
				t.Fatal("a sign-in Microsoft did not refresh was dropped")
			}
			app.mu.Lock()
			app.refreshErr = nil
			app.mu.Unlock()
			if _, err := keychain.Session(context.Background()); err != nil {
				t.Fatalf("session once Microsoft answers = %v", err)
			}
		})
	}
}

// A join reads the session before it dials, so a slow sign-in must not hold it past its own
// deadline: concurrent joins share one sign-in and each stops waiting at its deadline, while
// the sign-in finishes in the background for the joins after them.
func TestKeychainSessionWaitsOnlyAsLongAsItsContext(t *testing.T) {
	fake := newFakeServices(t)
	app := newFakeApp()
	keychain := newTestKeychain(t, app, fake.services())
	if _, err := keychain.BeginSignIn(context.Background()); err != nil {
		t.Fatal(err)
	}
	close(app.finish)
	testwait.Eventually(t, 5*time.Second, "the sign-in to finish", keychain.SignedIn)
	app.stall = make(chan struct{})

	for range 2 {
		ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
		_, err := keychain.Session(ctx)
		cancel()
		if !errors.Is(err, context.DeadlineExceeded) {
			t.Fatalf("a stalled sign-in returned %v", err)
		}
	}
	close(app.stall)
	session, err := keychain.Session(context.Background())
	if err != nil || session.Profile.Name != "Notch" {
		t.Fatalf("session after the stall = %+v, %v", session, err)
	}
	if _, identity := app.counts(); identity != 1 {
		t.Fatalf("the waiting joins started %d sign-ins", identity)
	}
}

// A closed keychain, as of a removed account, says so rather than reading as a cancelled join.
func TestKeychainClosedSessionSaysSo(t *testing.T) {
	keychain := newTestKeychain(t, newFakeApp(), newFakeServices(t).services())
	keychain.Close()
	if _, err := keychain.Session(context.Background()); !errors.Is(err, ErrKeychainClosed) {
		t.Fatalf("session of a closed keychain = %v", err)
	}
}

// A token file another sign-in replaced, as when the account is added again, is reopened by
// the next session rather than failing every join after it.
func TestKeychainReopensAReplacedToken(t *testing.T) {
	fake := newFakeServices(t)
	app := newFakeApp()
	close(app.finish)
	var offset atomic.Int64
	dir := t.TempDir()
	keychain := NewKeychain(KeychainConfig{
		Path: filepath.Join(dir, "token.json"), App: app, Services: fake.services(),
		Now: func() time.Time { return time.Now().Add(time.Duration(offset.Load())) },
	})
	t.Cleanup(keychain.Close)
	if _, err := keychain.BeginSignIn(context.Background()); err != nil {
		t.Fatal(err)
	}
	testwait.Eventually(t, 5*time.Second, "the sign-in to finish", keychain.SignedIn)
	if _, err := keychain.Session(context.Background()); err != nil {
		t.Fatal(err)
	}

	again := NewKeychain(KeychainConfig{Path: filepath.Join(dir, "again.json"), App: app, Services: fake.services()})
	t.Cleanup(again.Close)
	if _, err := again.BeginSignIn(context.Background()); err != nil {
		t.Fatal(err)
	}
	testwait.Eventually(t, 5*time.Second, "the second sign-in to finish", again.SignedIn)
	again.Close()
	if err := authcache.Move(context.Background(), again.config.Path, keychain.config.Path); err != nil {
		t.Fatal(err)
	}
	// The session expires, so the next one signs in with the replaced token.
	offset.Store(int64(48 * time.Hour))
	if _, err := keychain.Session(context.Background()); err != nil {
		t.Fatalf("session after the token was replaced = %v", err)
	}
}
