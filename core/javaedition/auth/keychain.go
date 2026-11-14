package auth

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"golang.org/x/oauth2"
)

// CacheFile is the name of the Java Edition sign-in's token file, kept beside the account's
// own Microsoft token.
const CacheFile = "java-edition-token.json"

// sessionMargin is how long before its expiry a Java session is replaced.
const sessionMargin = 10 * time.Minute

// ErrNotSignedIn reports that the account has no Java Edition sign-in yet.
var ErrNotSignedIn = errors.New("the account has no Java Edition sign-in")

// SignInRequired asks the player to finish the Java Edition sign-in: open VerificationURI and
// enter UserCode. The sign-in completes in the background, after which joins succeed.
type SignInRequired struct {
	VerificationURI string
	UserCode        string
}

func (e *SignInRequired) Error() string {
	return fmt.Sprintf("java edition sign-in pending at %s with code %s", e.VerificationURI, e.UserCode)
}

// MicrosoftApp is the Microsoft app the Java Edition sign-in goes through. The Java Edition
// services accept tokens only from apps registered with them, which the account's own
// Bedrock sign-in is not.
type MicrosoftApp interface {
	// DeviceAuth starts a device-code sign-in.
	DeviceAuth(ctx context.Context) (*oauth2.DeviceAuthResponse, error)
	// DeviceAccessToken waits for the player to finish a device-code sign-in.
	DeviceAccessToken(ctx context.Context, device *oauth2.DeviceAuthResponse) (*oauth2.Token, error)
	// TokenSource refreshes a token of this app.
	TokenSource(ctx context.Context, token *oauth2.Token) oauth2.TokenSource
	// IdentityToken returns the Xbox identity token for RelyingParty
	// ("XBL3.0 x=<user hash>;<token>") of the account behind source.
	IdentityToken(ctx context.Context, source oauth2.TokenSource) (string, error)
}

// SwitchApp is the Nintendo Switch edition's Microsoft app, which the Java Edition services
// accept.
func SwitchApp(client *http.Client) MicrosoftApp { return switchApp{client: client} }

type switchApp struct{ client *http.Client }

func (switchApp) DeviceAuth(ctx context.Context) (*oauth2.DeviceAuthResponse, error) {
	return auth.NintendoConfig.DeviceAuth(ctx)
}

func (switchApp) DeviceAccessToken(ctx context.Context, device *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
	return auth.NintendoConfig.DeviceAccessToken(ctx, device)
}

func (switchApp) TokenSource(ctx context.Context, token *oauth2.Token) oauth2.TokenSource {
	return auth.NintendoConfig.TokenSource(ctx, token)
}

func (a switchApp) IdentityToken(ctx context.Context, source oauth2.TokenSource) (string, error) {
	token, err := auth.NintendoConfig.New(source, &sisu.SessionConfig{HTTPClient: a.client}).XSTSToken(ctx, RelyingParty)
	if err != nil {
		return "", fmt.Errorf("java edition xbox token: %w", err)
	}
	return token.String(), nil
}

// KeychainConfig configures a Keychain.
type KeychainConfig struct {
	// Path is the token file; CachePath derives it from the account's token file.
	Path string
	// App is the Microsoft app of the sign-in; nil uses SwitchApp.
	App MicrosoftApp
	// Services are the Java Edition endpoints; the zero value uses the production services.
	Services Services
	Logger   *slog.Logger
	// Now reads the clock; nil uses time.Now.
	Now func() time.Time
}

// Keychain is one account's Java Edition sign-in: a Microsoft sign-in through an app the Java
// Edition services accept, kept in its own token file, and the Java session derived from it.
// The session is reused until shortly before it expires, since the services limit how often
// an account signs in.
type Keychain struct {
	config   KeychainConfig
	lifetime context.Context
	stop     context.CancelFunc

	mu      sync.Mutex
	source  oauth2.TokenSource
	session Session
	pending *pendingSignIn
}

// pendingSignIn is a device-code sign-in the player has not finished yet.
type pendingSignIn struct {
	prompt  SignInRequired
	expires time.Time
}

// CachePath returns the Java Edition token file of the account whose Microsoft token is at
// oauthPath.
func CachePath(oauthPath string) string {
	return filepath.Join(filepath.Dir(oauthPath), CacheFile)
}

// NewKeychain returns the Java Edition sign-in kept at config.Path. Close stops a sign-in
// still waiting for the player.
func NewKeychain(config KeychainConfig) *Keychain {
	if config.App == nil {
		config.App = SwitchApp(config.Services.HTTP)
	}
	if config.Logger == nil {
		config.Logger = slog.New(slog.DiscardHandler)
	}
	if config.Now == nil {
		config.Now = time.Now
	}
	lifetime, stop := context.WithCancel(context.Background())
	return &Keychain{config: config, lifetime: lifetime, stop: stop}
}

// Close stops any sign-in waiting for the player.
func (k *Keychain) Close() { k.stop() }

// SignedIn reports whether the account has a Java Edition sign-in.
func (k *Keychain) SignedIn() bool {
	k.mu.Lock()
	defer k.mu.Unlock()
	if k.source != nil {
		return true
	}
	_, err := os.Stat(k.config.Path)
	return err == nil
}

// Session returns the account's Java Edition session, signing in with the saved token when
// the last session is about to expire. It returns ErrNotSignedIn when there is no sign-in.
func (k *Keychain) Session(ctx context.Context) (Session, error) {
	k.mu.Lock()
	defer k.mu.Unlock()
	if k.session.AccessToken != "" && k.config.Now().Add(sessionMargin).Before(k.session.Expires) {
		return k.session, nil
	}
	if k.source == nil {
		if _, err := os.Stat(k.config.Path); errors.Is(err, os.ErrNotExist) {
			return Session{}, ErrNotSignedIn
		}
		source, err := authcache.Source(k.lifetime, authcache.Config{
			Path:    k.config.Path,
			Request: func(context.Context, io.Writer) (*oauth2.Token, error) { return nil, ErrNotSignedIn },
			Refresh: k.refresh,
		})
		if err != nil {
			return Session{}, fmt.Errorf("java edition sign-in: %w", err)
		}
		k.source = source
	}
	identity, err := k.config.App.IdentityToken(ctx, k.source)
	if err != nil {
		return Session{}, err
	}
	session, err := k.config.Services.SignIn(ctx, identity)
	if err != nil {
		return Session{}, err
	}
	k.session = session
	return session, nil
}

// BeginSignIn starts the device-code sign-in, or returns the one already waiting for the
// player. It finishes in the background and saves the token for later joins.
func (k *Keychain) BeginSignIn(ctx context.Context) (*SignInRequired, error) {
	k.mu.Lock()
	defer k.mu.Unlock()
	if k.pending != nil && k.config.Now().Before(k.pending.expires) {
		prompt := k.pending.prompt
		return &prompt, nil
	}
	device, err := k.config.App.DeviceAuth(ctx)
	if err != nil {
		return nil, fmt.Errorf("start java edition sign-in: %w", err)
	}
	pending := &pendingSignIn{
		prompt:  SignInRequired{VerificationURI: device.VerificationURI, UserCode: device.UserCode},
		expires: device.Expiry,
	}
	if pending.expires.IsZero() {
		pending.expires = k.config.Now().Add(15 * time.Minute)
	}
	k.pending = pending
	go k.finishSignIn(device, pending)
	prompt := pending.prompt
	return &prompt, nil
}

// finishSignIn waits for the player to finish a device-code sign-in and saves its token.
func (k *Keychain) finishSignIn(device *oauth2.DeviceAuthResponse, pending *pendingSignIn) {
	token, err := k.config.App.DeviceAccessToken(k.lifetime, device)
	if err == nil {
		var source oauth2.TokenSource
		source, err = authcache.Source(k.lifetime, authcache.Config{
			Path:    k.config.Path,
			Request: func(context.Context, io.Writer) (*oauth2.Token, error) { return token, nil },
			Refresh: k.refresh,
		})
		if err == nil {
			k.mu.Lock()
			k.source, k.session = source, Session{}
			k.mu.Unlock()
			k.config.Logger.Info("Java Edition sign-in finished")
		}
	}
	if err != nil && k.lifetime.Err() == nil {
		k.config.Logger.Warn("Java Edition sign-in did not finish", "error", err)
	}
	k.mu.Lock()
	if k.pending == pending {
		k.pending = nil
	}
	k.mu.Unlock()
}

// Join registers a server join for the session. A refused session is dropped, so the next
// join signs in again.
func (k *Keychain) Join(ctx context.Context, session Session, serverHash string) error {
	err := k.config.Services.Join(ctx, session, serverHash)
	var status *StatusError
	if errors.As(err, &status) && (status.Status == http.StatusUnauthorized || status.Status == http.StatusForbidden) {
		k.mu.Lock()
		if k.session.AccessToken == session.AccessToken {
			k.session = Session{}
		}
		k.mu.Unlock()
	}
	return err
}

// Forget removes the Java Edition sign-in, as signing out of the account does.
func (k *Keychain) Forget(ctx context.Context) error {
	k.mu.Lock()
	k.source, k.session, k.pending = nil, Session{}, nil
	k.mu.Unlock()
	return authcache.Remove(ctx, k.config.Path, os.Remove)
}

// refresh is the token cache's refresh source for the sign-in's app.
func (k *Keychain) refresh(token *oauth2.Token, _ io.Writer) oauth2.TokenSource {
	return k.config.App.TokenSource(k.lifetime, token)
}
