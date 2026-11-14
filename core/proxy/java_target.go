package proxy

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"strings"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
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

// dialJavaUpstream joins a Java server. A signed-in account joins as its Java Edition
// profile, which online-mode servers require; without one the player joins under name and
// only offline-mode servers accept the login.
func dialJavaUpstream(ctx context.Context, address string, account *authcache.Account, name string, client *http.Client, logger *slog.Logger) (upstreamSession, error) {
	options := translate.Options{Username: name, Logger: logger}
	if options.Username == "" {
		options.Username = javaSignedOutName
	}
	signInErr := errors.New("joining an online-mode Java server needs a signed-in Microsoft account")
	if account != nil {
		services := auth.Services{HTTP: client}
		session, err := javaSignIn(ctx, account, services)
		if err == nil {
			options.Username = session.Profile.Name
			options.JoinSession = func(ctx context.Context, serverHash string) error {
				return services.Join(ctx, session, serverHash)
			}
		} else {
			// An offline-mode server never asks for the session, so the failure only
			// matters if this one does.
			logger.Warn("Java Edition sign-in failed; joining without a session", "error", err)
			signInErr = err
		}
	}
	if options.JoinSession == nil {
		options.JoinSession = func(context.Context, string) error { return signInErr }
	}
	session, err := translate.Dial(ctx, address, options)
	if err != nil {
		return nil, err
	}
	return javaUpstream{session}, nil
}

// javaSignIn signs the account in to the Java Edition services with its Xbox token.
func javaSignIn(ctx context.Context, account *authcache.Account, services auth.Services) (auth.Session, error) {
	token, err := account.XSTSToken(ctx, auth.RelyingParty)
	if err != nil {
		return auth.Session{}, fmt.Errorf("java edition xbox token: %w", err)
	}
	return services.SignIn(ctx, token.String())
}
