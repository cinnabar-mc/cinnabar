package auth

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"os"
	"sync"
	"sync/atomic"
	"time"

	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"golang.org/x/oauth2"
)

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

// ErrServicesUnreachable reports that Microsoft did not answer the refresh of a sign-in that
// may still be valid, so the player should retry rather than sign in again.
var ErrServicesUnreachable = errors.New("microsoft did not answer the Java Edition sign-in refresh")

// ErrKeychainClosed reports a keychain closed before or during the request, as when its
// account was removed or replaced.
var ErrKeychainClosed = errors.New("the Java Edition sign-in was closed")

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
	// Path is the token file.
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
	// OnSignedIn, when set, runs on its own goroutine once a device-code sign-in's token is
	// saved.
	OnSignedIn func()
	// OnSignInFailed, when set, runs on its own goroutine when a device-code sign-in ends
	// without a saved token for a reason other than being stopped, such as an expired or
	// declined code or a token that could not be saved. code is the failed sign-in's user
	// code, so a caller can tell it from a newer sign-in.
	OnSignInFailed func(code string, err error)
}

// Keychain is one account's Java Edition sign-in: a Microsoft sign-in through an app the Java
// Edition services accept, kept in its own token file, and the Java session derived from it.
// The session is reused until shortly before it expires, since the services limit how often
// an account signs in.
//
// mu guards the fields and is never held across a network call or a token file's lease.
type Keychain struct {
	config   KeychainConfig
	lifetime context.Context
	stop     context.CancelFunc
	// work counts the sign-ins running in the background: device-code flows and session
	// sign-ins.
	work sync.WaitGroup

	mu      sync.Mutex
	closed  bool
	source  oauth2.TokenSource
	session Session
	pending *pendingSignIn
	// signingIn is the session sign-in under way, which concurrent joins share.
	signingIn *sessionSignIn
	// generation changes whenever the sign-in is replaced, dropped or forgotten, so work
	// begun before cannot store a stale result.
	generation uint64
	// forgets counts sign-outs, which a token saved meanwhile must not survive.
	forgets uint64
	// unusable marks a saved token that no longer works, until a new sign-in replaces it.
	unusable bool
	// lastRefresh is the last failed token refresh of the session sign-in under way, which
	// tells a Microsoft that did not answer from a refused sign-in.
	lastRefresh atomic.Pointer[error]
}

// pendingSignIn is a device-code sign-in the player has not finished yet.
type pendingSignIn struct {
	prompt  SignInRequired
	expires time.Time
	cancel  context.CancelFunc
}

// sessionSignIn is one session sign-in; done closes once session and err are set.
type sessionSignIn struct {
	done    chan struct{}
	session Session
	err     error
}

// NewKeychain returns the Java Edition sign-in kept at config.Path. Close stops the sign-ins
// running in the background.
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

// Close stops the sign-ins running in the background and waits for them to end. No sign-in
// starts afterwards.
func (k *Keychain) Close() {
	k.mu.Lock()
	k.closed = true
	k.mu.Unlock()
	k.stop()
	k.work.Wait()
}

// SignedIn reports whether the account has a saved Java Edition sign-in that has not yet
// turned out unusable.
func (k *Keychain) SignedIn() bool {
	k.mu.Lock()
	defer k.mu.Unlock()
	if k.source != nil {
		return true
	}
	if k.unusable {
		return false
	}
	_, err := os.Stat(k.config.Path)
	return err == nil
}

// Session returns the account's Java Edition session, signing in with the saved token when
// the last session is about to expire. Concurrent callers share one sign-in, and each waits
// only as long as its own ctx allows; the sign-in itself is bounded by the HTTP client's
// timeout. It returns ErrNotSignedIn when there is no usable sign-in, so the caller can
// start one.
func (k *Keychain) Session(ctx context.Context) (Session, error) {
	k.mu.Lock()
	if k.session.AccessToken != "" && k.config.Now().Add(sessionMargin).Before(k.session.Expires) {
		session := k.session
		k.mu.Unlock()
		return session, nil
	}
	if k.closed {
		k.mu.Unlock()
		return Session{}, ErrKeychainClosed
	}
	if k.unusable && k.source == nil {
		k.mu.Unlock()
		return Session{}, fmt.Errorf("%w: the saved one no longer works", ErrNotSignedIn)
	}
	call := k.signingIn
	if call == nil {
		call = &sessionSignIn{done: make(chan struct{})}
		k.signingIn = call
		k.work.Add(1)
		go k.signInSession(call, k.source, k.generation)
	}
	k.mu.Unlock()
	select {
	case <-call.done:
		if call.err != nil && k.lifetime.Err() != nil {
			// The keychain closed under the sign-in, as when its account was removed.
			return Session{}, fmt.Errorf("%w: %w", ErrKeychainClosed, call.err)
		}
		return call.session, call.err
	case <-ctx.Done():
		return Session{}, ctx.Err()
	}
}

// signInSession signs in with the saved token and stores the session, unless the sign-in
// was replaced meanwhile.
func (k *Keychain) signInSession(call *sessionSignIn, source oauth2.TokenSource, generation uint64) {
	defer k.work.Done()
	opened := source
	session, source, err := k.newSession(source)
	if opened != nil && authcache.AccountChanged(err) {
		// Another sign-in, in this core or another, replaced the token file: open the new one.
		session, source, err = k.newSession(nil)
	}
	k.mu.Lock()
	if k.generation == generation {
		switch {
		case err == nil:
			k.source, k.session = source, session
		case errors.Is(err, ErrNotSignedIn):
			k.source, k.session, k.unusable = nil, Session{}, true
			k.generation++
		}
	}
	k.signingIn = nil
	k.mu.Unlock()
	call.session, call.err = session, err
	close(call.done)
}

// newSession signs in to the Java Edition services with source, opening the saved token
// when source is nil. An unreadable, unrefreshable or refused token is ErrNotSignedIn.
func (k *Keychain) newSession(source oauth2.TokenSource) (Session, oauth2.TokenSource, error) {
	ctx := k.lifetime
	k.lastRefresh.Store(nil)
	if source == nil {
		if _, err := os.Stat(k.config.Path); errors.Is(err, os.ErrNotExist) {
			return Session{}, nil, ErrNotSignedIn
		}
		// The token cache asks for a new token when the saved one is unreadable or did not
		// refresh. Only an unreadable token or a refusal means the player has to sign in
		// again; a refresh Microsoft did not answer leaves the token for a later join.
		var err error
		source, err = authcache.Source(ctx, authcache.Config{
			Path: k.config.Path,
			Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
				if failure := k.lastRefresh.Load(); failure != nil && !refusedByMicrosoft(*failure) {
					return nil, fmt.Errorf("%w: %w", ErrServicesUnreachable, *failure)
				}
				return nil, ErrNotSignedIn
			},
			Refresh: k.refresh,
		})
		if errors.Is(err, ErrNotSignedIn) {
			return Session{}, nil, fmt.Errorf("%w: the saved one no longer refreshes", ErrNotSignedIn)
		}
		if err != nil {
			return Session{}, nil, fmt.Errorf("java edition sign-in: %w", err)
		}
	}
	identity, err := k.config.App.IdentityToken(ctx, source)
	switch {
	case err == nil:
	case refusedByMicrosoft(err):
		// The Microsoft sign-in itself was refused, as for a revoked token.
		return Session{}, nil, fmt.Errorf("%w: %v", ErrNotSignedIn, err)
	case k.lastRefresh.Load() != nil && !refusedByMicrosoft(*k.lastRefresh.Load()) && !errors.Is(err, ErrNotSignedIn):
		// A refresh of the token Microsoft did not answer leaves the token usable.
		return Session{}, nil, fmt.Errorf("%w: %w", ErrServicesUnreachable, err)
	default:
		return Session{}, nil, err
	}
	session, err := k.config.Services.SignIn(ctx, identity)
	if err != nil {
		return Session{}, nil, err
	}
	return session, source, nil
}

// BeginSignIn starts the device-code sign-in, or returns the one already waiting for the
// player. It finishes in the background and saves the token for later joins.
func (k *Keychain) BeginSignIn(ctx context.Context) (*SignInRequired, error) {
	if prompt := k.livePrompt(); prompt != nil {
		return prompt, nil
	}
	k.mu.Lock()
	forgets := k.forgets
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
	switch {
	case k.closed || k.forgets != forgets:
		// Closed or signed out while the code was requested.
		k.mu.Unlock()
		cancel()
		return nil, fmt.Errorf("%w: the account signed out", ErrSignInUnavailable)
	case k.pending != nil && k.config.Now().Before(k.pending.expires):
		// Another join started a sign-in meanwhile; its code is the one to enter.
		prompt := k.pending.prompt
		k.mu.Unlock()
		cancel()
		return &prompt, nil
	}
	if k.pending != nil {
		k.pending.cancel()
	}
	k.pending = pending
	k.work.Add(1)
	k.mu.Unlock()
	go k.finishSignIn(flow, device, pending)
	prompt := pending.prompt
	return &prompt, nil
}

// livePrompt returns the code of the sign-in waiting for the player, if it is still valid.
func (k *Keychain) livePrompt() *SignInRequired {
	k.mu.Lock()
	defer k.mu.Unlock()
	if k.pending == nil || !k.config.Now().Before(k.pending.expires) {
		return nil
	}
	prompt := k.pending.prompt
	return &prompt
}

// finishSignIn waits for the player to finish a device-code sign-in and saves its token,
// unless the sign-in was forgotten or replaced meanwhile.
func (k *Keychain) finishSignIn(flow context.Context, device *oauth2.DeviceAuthResponse, pending *pendingSignIn) {
	defer k.work.Done()
	defer pending.cancel()
	token, err := k.config.App.DeviceAccessToken(flow, device)
	k.mu.Lock()
	if k.pending != pending {
		k.mu.Unlock()
		return
	}
	k.pending = nil
	forgets := k.forgets
	k.mu.Unlock()
	if err != nil {
		if flow.Err() == nil {
			k.config.Logger.Warn("Java Edition sign-in did not finish", "error", err)
			if k.config.OnSignInFailed != nil {
				go k.config.OnSignInFailed(pending.prompt.UserCode, err)
			}
		}
		return
	}
	// Any older token goes first, so the cache saves this one instead of refreshing that.
	err = authcache.Remove(k.lifetime, k.config.Path, os.Remove)
	var source oauth2.TokenSource
	if err == nil {
		source, err = authcache.Source(k.lifetime, authcache.Config{
			Path:    k.config.Path,
			Request: func(context.Context, io.Writer) (*oauth2.Token, error) { return token, nil },
			Refresh: k.refresh,
		})
	}
	if err != nil {
		k.config.Logger.Warn("Java Edition sign-in could not be saved", "error", err)
		if k.config.OnSignInFailed != nil {
			go k.config.OnSignInFailed(pending.prompt.UserCode, err)
		}
		return
	}
	k.mu.Lock()
	forgotten := k.forgets != forgets
	if !forgotten {
		k.source, k.session, k.unusable = source, Session{}, false
		k.generation++
	}
	k.mu.Unlock()
	if forgotten {
		// The account signed out while the token was saved.
		_ = authcache.Remove(k.lifetime, k.config.Path, os.Remove)
		return
	}
	k.config.Logger.Info("Java Edition sign-in finished")
	if k.config.OnSignedIn != nil {
		go k.config.OnSignedIn()
	}
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
	k.source, k.session, k.pending, k.unusable = nil, Session{}, nil, false
	k.generation++
	k.forgets++
	k.mu.Unlock()
	return authcache.Remove(ctx, k.config.Path, os.Remove)
}

// refresh is the token cache's refresh source for the sign-in's app.
func (k *Keychain) refresh(token *oauth2.Token, _ io.Writer) oauth2.TokenSource {
	return recordingSource{source: k.config.App.TokenSource(k.lifetime, token), last: &k.lastRefresh}
}

// recordingSource keeps the last refresh failure, which the token cache does not pass on.
type recordingSource struct {
	source oauth2.TokenSource
	last   *atomic.Pointer[error]
}

func (s recordingSource) Token() (*oauth2.Token, error) {
	token, err := s.source.Token()
	if err != nil {
		s.last.Store(&err)
	}
	return token, err
}

// refusedByMicrosoft reports whether Microsoft answered a token request with a refusal, such
// as for a revoked or expired sign-in, rather than failing to answer it.
func refusedByMicrosoft(err error) bool {
	var refused *oauth2.RetrieveError
	if !errors.As(err, &refused) {
		return false
	}
	if refused.Response == nil {
		return true
	}
	switch status := refused.Response.StatusCode; {
	case status == http.StatusRequestTimeout, status == http.StatusTooManyRequests:
		// A timeout or throttling says nothing about the sign-in itself.
		return false
	default:
		return status < http.StatusInternalServerError
	}
}
