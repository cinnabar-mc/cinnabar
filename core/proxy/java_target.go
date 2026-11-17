package proxy

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strings"

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

func (javaUpstream) ResourcePackOffer() (minecraft.ResourcePackOfferSnapshot, bool) {
	return minecraft.ResourcePackOfferSnapshot{}, true
}

func (javaUpstream) ResourcePackStack() (minecraft.ResourcePackStackSnapshot, bool) {
	return minecraft.ResourcePackStackSnapshot{}, true
}

// dialJavaUpstream joins a Java server. The selected Java account logs in as its saved
// profile, which online-mode servers require. Without one the player joins under name, and an
// online-mode server's request for the session starts adding an account and fails the join
// with the code to enter, as does a selected account whose sign-in no longer works. Failures
// the player can act on reach the client as disconnect text.
func dialJavaUpstream(ctx context.Context, address string, accounts *auth.Accounts, name string, logger *slog.Logger) (upstreamSession, error) {
	options := translate.Options{Username: name, Logger: logger}
	if options.Username == "" {
		options.Username = javaSignedOutName
	}
	options.JoinSession = func(context.Context, string) error { return errJavaNeedsAccount }
	if accounts != nil {
		addAccount := func(ctx context.Context) error {
			prompt, err := accounts.BeginAdd(ctx)
			if err != nil {
				return err
			}
			return prompt
		}
		options.JoinSession = func(ctx context.Context, _ string) error { return addAccount(ctx) }
		if account, keychain, ok := accounts.Selected(); ok {
			options.Username = account.Name
			options.JoinSession = func(ctx context.Context, serverHash string) error {
				session, err := keychain.Session(ctx)
				if errors.Is(err, auth.ErrNotSignedIn) {
					return addAccount(ctx)
				}
				if errors.Is(err, auth.ErrKeychainClosed) {
					return errJavaAccountChanged
				}
				if err != nil {
					return err
				}
				if session.Profile.Name != account.Name {
					// The profile was renamed since it was saved, and the server checks the join
					// against the name the login sent.
					if err := accounts.Rename(account.ID, session.Profile.Name); err != nil {
						logger.Warn("Java Edition account name not updated", "error", err)
					}
					return errJavaProfileRenamed
				}
				return keychain.Join(ctx, session, serverHash)
			}
		}
	}
	session, err := translate.Dial(ctx, address, options)
	if err != nil {
		return nil, javaJoinError(err)
	}
	return javaUpstream{session}, nil
}

// errJavaProfileRenamed refuses a join whose login sent the account's old profile name; the
// saved name is updated, so the next join sends the new one.
var errJavaProfileRenamed = errors.New("the Java Edition profile was renamed since it was saved")

// errJavaAccountChanged refuses a join whose account was removed or replaced while it signed
// in; the next join uses the account selected then.
var errJavaAccountChanged = errors.New("the Java Edition account changed during the join")

// errJavaNeedsAccount refuses an online-mode join of a player who is not signed in.
var errJavaNeedsAccount = errors.New("joining an online-mode Java server needs a Java Edition account")

// javaJoinError gives a failed authentication the disconnect text that tells the player what
// to do; any other failure keeps its error.
func javaJoinError(err error) error {
	var prompt *auth.SignInRequired
	var status *auth.StatusError
	var message string
	switch {
	case errors.As(err, &prompt):
		message = fmt.Sprintf("Add a Java account to join this server: go to %s, enter %s, then reconnect.", prompt.VerificationURI, prompt.UserCode)
	case errors.Is(err, errJavaNeedsAccount):
		message = "This Java server needs a Java account. Add one in Accounts, then reconnect."
	case errors.Is(err, errJavaAccountChanged):
		message = "Your Java account changed. Reconnect to join."
	case errors.Is(err, errJavaProfileRenamed):
		message = "Your Java profile was renamed since it was added. Reconnect to join."
	case errors.Is(err, auth.ErrAddInProgress):
		message = "Your Java account is being added. Reconnect in a moment."
	case errors.Is(err, auth.ErrServicesUnreachable):
		message = "Could not reach Microsoft to sign in to Java Edition. Reconnect to try again."
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
