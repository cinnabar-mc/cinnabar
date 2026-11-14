package proxy

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/auth"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/translate"
	"github.com/sandertv/gophertunnel/minecraft"
)

// JavaTargetPrefix marks a target as a Java Edition 1.7.10 server: "java/<host>[:<port>]".
const JavaTargetPrefix = bridgecontract.JavaTargetPrefix

// javaSignedOutName is the login name of a signed-out player that names none.
const javaSignedOutName = "Player"

// IsJavaTarget reports whether address names a Java Edition server.
func IsJavaTarget(address string) bool {
	_, ok := cutPrefixFold(strings.TrimSpace(address), JavaTargetPrefix)
	return ok
}

// ValidJavaTarget reports whether address is a Java target that names a server.
func ValidJavaTarget(address string) bool {
	server, ok := cutPrefixFold(strings.TrimSpace(address), JavaTargetPrefix)
	if !ok {
		return false
	}
	_, err := javaTarget(server)
	return err == nil
}

// javaTarget resolves a Java server address. The port is optional: the Java address rules,
// including the service record lookup, apply when the server is dialled.
func javaTarget(server string) (*resolvedUpstreamTarget, error) {
	server = strings.TrimSpace(server)
	if server == "" || strings.ContainsAny(server, " \t/\\") {
		return nil, fmt.Errorf("invalid Java target %q: want %s<host>[:<port>]", server, JavaTargetPrefix)
	}
	return &resolvedUpstreamTarget{address: server, java: true}, nil
}

// javaUpstream is a translated Java session with the empty pack offer and stack a Bedrock
// upstream reports when a server has no packs.
type javaUpstream struct {
	*translate.Session
}

// Edition names the server's edition for the session handoff.
func (javaUpstream) Edition() string {
	return bridgecontract.JavaEdition
}

func (javaUpstream) ResourcePackOffer() (minecraft.ResourcePackOfferSnapshot, bool) {
	return minecraft.ResourcePackOfferSnapshot{}, true
}

func (javaUpstream) ResourcePackStack() (minecraft.ResourcePackStackSnapshot, bool) {
	return minecraft.ResourcePackStackSnapshot{}, true
}

// dialJavaUpstream joins a Java server. With a Java Edition sign-in the player joins as its
// profile, which online-mode servers require; without a usable one the player joins under
// name, and an online-mode server's request for the session starts the sign-in and fails the
// join with the code to enter. Failures the player can act on reach the client as disconnect
// text.
func dialJavaUpstream(ctx context.Context, address string, keychain *auth.Keychain, name string, logger *slog.Logger) (upstreamSession, error) {
	options := translate.Options{Username: name, Logger: logger}
	if options.Username == "" {
		options.Username = javaSignedOutName
	}
	options.JoinSession = func(context.Context, string) error { return errJavaNeedsAccount }
	if keychain != nil {
		// Only an online-mode server asks for the session. The join's own context bounds the
		// sign-in then, and no usable sign-in starts a new one.
		options.JoinSession = func(ctx context.Context, serverHash string) error {
			session, err := keychain.Session(ctx)
			if errors.Is(err, auth.ErrNotSignedIn) {
				prompt, err := keychain.BeginSignIn(ctx)
				if err != nil {
					return err
				}
				return prompt
			}
			if err != nil {
				return err
			}
			if session.Profile.Name != options.Username {
				// The sign-in finished after the login named the player, and the server checks
				// the join against that name.
				return errJavaSignInLate
			}
			return keychain.Join(ctx, session, serverHash)
		}
		if keychain.SignedIn() {
			// The login names the profile, so the session is read before dialing. The wait is
			// bounded, so an offline-mode server's join never waits long on the services.
			signInCtx, cancel := context.WithTimeout(ctx, javaSignInTimeout)
			session, err := keychain.Session(signInCtx)
			cancel()
			if err == nil {
				options.Username = session.Profile.Name
			} else {
				logger.Warn("Java Edition sign-in unavailable before the join", "error", err)
			}
		}
	}
	session, err := translate.Dial(ctx, address, options)
	if err != nil {
		return nil, javaJoinError(err)
	}
	return javaUpstream{session}, nil
}

// javaSignInTimeout bounds the sign-in a Java join makes before it dials.
const javaSignInTimeout = 10 * time.Second

// errJavaSignInLate refuses a join whose Java Edition sign-in finished only after the login
// had named the player; the next join uses the finished sign-in.
var errJavaSignInLate = errors.New("the Java Edition sign-in finished after the login named the player")

// errJavaNeedsAccount refuses an online-mode join of a player who is not signed in.
var errJavaNeedsAccount = errors.New("joining an online-mode Java server needs a signed-in Microsoft account")

// javaJoinError gives a failed authentication the disconnect text that tells the player what
// to do; any other failure keeps its error.
func javaJoinError(err error) error {
	var prompt *auth.SignInRequired
	var status *auth.StatusError
	var message string
	switch {
	case errors.As(err, &prompt):
		message = fmt.Sprintf("Java Edition needs its own Microsoft sign-in. Go to %s, enter %s, then reconnect.", prompt.VerificationURI, prompt.UserCode)
	case errors.Is(err, errJavaNeedsAccount):
		message = "This Java server needs a Java Edition account. Sign in to Cinnabar, then reconnect."
	case errors.Is(err, errJavaSignInLate):
		message = "The Java Edition sign-in was still finishing. Reconnect to join."
	case errors.Is(err, auth.ErrSignInUnavailable):
		message = "The Java Edition sign-in could not be started. Reconnect to try again."
	case errors.As(err, &status) && status.Operation == auth.OperationJoin:
		message = "The Java Edition session service refused the join: " + status.Error()
	case errors.As(err, &status):
		message = "Could not sign in to Java Edition: " + status.Error()
	default:
		return err
	}
	return errors.Join(&minecraft.DisconnectPacketError{Message: message}, err)
}
