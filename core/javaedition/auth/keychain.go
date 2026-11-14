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

// deviceCodeLifetime is how long a device code is taken to stay valid when the sign-in
// service names no expiry; Microsoft's codes last this long.
const deviceCodeLifetime = 15 * time.Minute

// defaultHTTPTimeout bounds one request of the sign-in when the caller supplies no client.
const defaultHTTPTimeout = 30 * time.Second

// ErrNotSignedIn reports that the account has no usable Java Edition sign-in: none was made,
// or the saved one can no longer be refreshed.
var ErrNotSignedIn = errors.New("the account has no Java Edition sign-in")

// ErrSignInUnavailable reports that the device-code sign-in could not be started.
var ErrSignInUnavailable = errors.New("the Java Edition sign-in could not be started")

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
// Bedrock sign-in is not. Network calls read their HTTP client from the context's
// oauth2.HTTPClient value.
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
func SwitchApp() MicrosoftApp { return switchApp{} }

type switchApp struct{}

func (switchApp) DeviceAuth(ctx context.Context) (*oauth2.DeviceAuthResponse, error) {
	return auth.NintendoConfig.DeviceAuth(ctx)
}

func (switchApp) DeviceAccessToken(ctx context.Context, device *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
	return auth.NintendoConfig.DeviceAccessToken(ctx, device)
}

func (switchApp) TokenSource(ctx context.Context, token *oauth2.Token) oauth2.TokenSource {
	return auth.NintendoConfig.TokenSource(ctx, token)
}

func (switchApp) IdentityToken(ctx context.Context, source oauth2.TokenSource) (string, error) {
	client, _ := ctx.Value(oauth2.HTTPClient).(*http.Client)
	token, err := auth.NintendoConfig.New(source, &sisu.SessionConfig{HTTPClient: client}).XSTSToken(ctx, RelyingParty)
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
	// Their HTTP client also carries the Microsoft sign-in's requests; nil uses one with a
	// timeout.
	Services Services
	Logger   *slog.Logger
	// Now reads the clock; nil uses time.Now.
	Now func() time.Time
}

// Keychain is one account's Java Edition sign-in: a Microsoft sign-in through an app the Java
// Edition services accept, kept in its own token file, and the Java session derived from it.
// The session is reused until shortly before it expires, since the services limit how often
// an account signs in.
//
// mu guards the fields and is never held across a network call. signIn serializes the
// network work of Session, so concurrent joins sign in once.
type Keychain struct {
	config   KeychainConfig
	lifetime context.Context
	stop     context.CancelFunc
	// flows counts the device-code sign-ins still running in the background.
	flows sync.WaitGroup

	signIn sync.Mutex

	mu      sync.Mutex
	source  oauth2.TokenSource
	session Session
	pending *pendingSignIn
	// generation changes whenever the sign-in is forgotten, so work begun before cannot
	// restore it.
	generation uint64
}

// pendingSignIn is a device-code sign-in the player has not finished yet.
type pendingSignIn struct {
	prompt  SignInRequired
	expires time.Time
	cancel  context.CancelFunc
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
		config.App = SwitchApp()
	}
	if config.Logger == nil {
		config.Logger = slog.New(slog.DiscardHandler)
	}
	if config.Now == nil {
		config.Now = time.Now
	}
	if config.Services.HTTP == nil {
		config.Services.HTTP = &http.Client{Timeout: defaultHTTPTimeout}
	}
	if config.Services.Now == nil {
		config.Services.Now = config.Now
	}
	lifetime, stop := context.WithCancel(context.WithValue(context.Background(), oauth2.HTTPClient, config.Services.HTTP))
	return &Keychain{config: config, lifetime: lifetime, stop: stop}
}

// Close stops any sign-in waiting for the player and waits for it to end.
func (k *Keychain) Close() {
	k.stop()
	k.flows.Wait()
}

// SignedIn reports whether the account has a saved Java Edition sign-in. It may still turn
// out unusable, which Session reports as ErrNotSignedIn.
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
// the last session is about to expire. It returns ErrNotSignedIn when there is no usable
// sign-in, so the caller can start one.
func (k *Keychain) Session(ctx context.Context) (Session, error) {
	k.signIn.Lock()
	defer k.signIn.Unlock()
	k.mu.Lock()
	session, source, generation := k.session, k.source, k.generation
	k.mu.Unlock()
	if session.AccessToken != "" && k.config.Now().Add(sessionMargin).Before(session.Expires) {
		return session, nil
	}
	ctx = context.WithValue(ctx, oauth2.HTTPClient, k.config.Services.HTTP)
	if source == nil {
		if _, err := os.Stat(k.config.Path); errors.Is(err, os.ErrNotExist) {
			return Session{}, ErrNotSignedIn
		}
		// The token cache asks for a new token when the saved one is unreadable or no longer
		// refreshes; the player has to sign in again then.
		var err error
		source, err = authcache.Source(k.lifetime, authcache.Config{
			Path:    k.config.Path,
			Request: func(context.Context, io.Writer) (*oauth2.Token, error) { return nil, ErrNotSignedIn },
			Refresh: k.refresh,
		})
		if errors.Is(err, ErrNotSignedIn) {
			return Session{}, fmt.Errorf("%w: the saved one no longer refreshes", ErrNotSignedIn)
		}
		if err != nil {
			return Session{}, fmt.Errorf("java edition sign-in: %w", err)
		}
	}
	identity, err := k.config.App.IdentityToken(ctx, source)
	var refused *oauth2.RetrieveError
	if errors.As(err, &refused) {
		// The Microsoft sign-in itself was refused, as for a revoked token.
		k.dropSource(source)
		return Session{}, fmt.Errorf("%w: %v", ErrNotSignedIn, err)
	}
	if err != nil {
		return Session{}, err
	}
	session, err = k.config.Services.SignIn(ctx, identity)
	if err != nil {
		return Session{}, err
	}
	k.mu.Lock()
	defer k.mu.Unlock()
	if k.generation == generation {
		k.source, k.session = source, session
	}
	return session, nil
}

// dropSource forgets a token source that no longer works, unless it was already replaced.
func (k *Keychain) dropSource(source oauth2.TokenSource) {
	k.mu.Lock()
	defer k.mu.Unlock()
	if k.source == source {
		k.source, k.session = nil, Session{}
	}
}

// BeginSignIn starts the device-code sign-in, or returns the one already waiting for the
// player. It finishes in the background and saves the token for later joins.
func (k *Keychain) BeginSignIn(ctx context.Context) (*SignInRequired, error) {
	k.mu.Lock()
	if k.pending != nil && k.config.Now().Before(k.pending.expires) {
		prompt := k.pending.prompt
		k.mu.Unlock()
		return &prompt, nil
	}
	generation := k.generation
	k.mu.Unlock()

	device, err := k.config.App.DeviceAuth(context.WithValue(ctx, oauth2.HTTPClient, k.config.Services.HTTP))
	if err != nil {
		return nil, fmt.Errorf("%w: %v", ErrSignInUnavailable, err)
	}
	flow, cancel := context.WithCancel(k.lifetime)
	pending := &pendingSignIn{
		prompt:  SignInRequired{VerificationURI: device.VerificationURI, UserCode: device.UserCode},
		expires: device.Expiry,
		cancel:  cancel,
	}
	if pending.expires.IsZero() {
		pending.expires = k.config.Now().Add(deviceCodeLifetime)
	}
	k.mu.Lock()
	if k.generation != generation {
		// Forgotten while the code was requested.
		k.mu.Unlock()
		cancel()
		return nil, fmt.Errorf("%w: the account signed out", ErrSignInUnavailable)
	}
	if k.pending != nil {
		k.pending.cancel()
	}
	k.pending = pending
	k.flows.Add(1)
	k.mu.Unlock()
	go k.finishSignIn(flow, device, pending)
	prompt := pending.prompt
	return &prompt, nil
}

// finishSignIn waits for the player to finish a device-code sign-in and saves its token,
// unless the sign-in was forgotten or replaced meanwhile.
func (k *Keychain) finishSignIn(flow context.Context, device *oauth2.DeviceAuthResponse, pending *pendingSignIn) {
	defer k.flows.Done()
	defer pending.cancel()
	token, err := k.config.App.DeviceAccessToken(flow, device)
	k.mu.Lock()
	defer k.mu.Unlock()
	if k.pending != pending {
		return
	}
	k.pending = nil
	if err != nil {
		if flow.Err() == nil {
			k.config.Logger.Warn("Java Edition sign-in did not finish", "error", err)
		}
		return
	}
	// Saving needs no network: the token cache writes the requested token.
	source, err := authcache.Source(k.lifetime, authcache.Config{
		Path:    k.config.Path,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) { return token, nil },
		Refresh: k.refresh,
	})
	if err != nil {
		k.config.Logger.Warn("Java Edition sign-in could not be saved", "error", err)
		return
	}
	k.source, k.session = source, Session{}
	k.config.Logger.Info("Java Edition sign-in finished")
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

// Forget removes the Java Edition sign-in, as signing out of the account does, and stops a
// sign-in still waiting for the player.
func (k *Keychain) Forget(ctx context.Context) error {
	k.mu.Lock()
	if k.pending != nil {
		k.pending.cancel()
	}
	k.source, k.session, k.pending = nil, Session{}, nil
	k.generation++
	k.mu.Unlock()
	return authcache.Remove(ctx, k.config.Path, os.Remove)
}

// refresh is the token cache's refresh source for the sign-in's app.
func (k *Keychain) refresh(token *oauth2.Token, _ io.Writer) oauth2.TokenSource {
	return k.config.App.TokenSource(k.lifetime, token)
}
