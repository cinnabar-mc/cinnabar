// Package auth signs a Microsoft account in to the Java Edition services and registers server
// joins with the session service, which online-mode servers check.
package auth

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
)

// RelyingParty is the Xbox relying party the Java Edition services accept tokens for.
const RelyingParty = "rp://api.minecraftservices.com/"

// Default service endpoints.
const (
	defaultLoginURL   = "https://api.minecraftservices.com/authentication/login_with_xbox"
	defaultProfileURL = "https://api.minecraftservices.com/minecraft/profile"
	defaultJoinURL    = "https://sessionserver.mojang.com/session/minecraft/join"
)

// maxResponseBytes bounds a service response body.
const maxResponseBytes = 1 << 20

// ErrNoProfile reports an account that has no Java Edition profile: it does not own the game
// or has not chosen a name.
var ErrNoProfile = errors.New("the Microsoft account has no Java Edition profile")

// Services are the Java Edition endpoints. The zero value uses the production services and
// the default HTTP client.
type Services struct {
	HTTP       *http.Client
	LoginURL   string
	ProfileURL string
	JoinURL    string
}

// Profile is a Java Edition player: its id, as 32 hexadecimal digits, and its name.
type Profile struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

// Session is a signed-in Java Edition account. AccessToken is secret.
type Session struct {
	AccessToken string
	Profile     Profile
}

// StatusError is a service response outside the 2xx range.
type StatusError struct {
	Operation string
	Status    int
}

func (e *StatusError) Error() string {
	return fmt.Sprintf("java edition %s: HTTP %d", e.Operation, e.Status)
}

// SignIn exchanges an Xbox identity token ("XBL3.0 x=<user hash>;<token>") for the relying
// party RelyingParty for a Java Edition session and reads the account's profile.
func (s Services) SignIn(ctx context.Context, identityToken string) (Session, error) {
	body, err := json.Marshal(map[string]string{"identityToken": identityToken})
	if err != nil {
		return Session{}, err
	}
	var login struct {
		AccessToken string `json:"access_token"`
	}
	if err := s.call(ctx, "sign-in", http.MethodPost, orDefault(s.LoginURL, defaultLoginURL), "", body, &login); err != nil {
		return Session{}, err
	}
	if login.AccessToken == "" {
		return Session{}, errors.New("java edition sign-in: response has no access token")
	}
	session := Session{AccessToken: login.AccessToken}
	err = s.call(ctx, "profile", http.MethodGet, orDefault(s.ProfileURL, defaultProfileURL), login.AccessToken, nil, &session.Profile)
	var status *StatusError
	if errors.As(err, &status) && status.Status == http.StatusNotFound {
		return Session{}, ErrNoProfile
	}
	if err != nil {
		return Session{}, err
	}
	if session.Profile.ID == "" || session.Profile.Name == "" {
		return Session{}, ErrNoProfile
	}
	return session, nil
}

// Join registers that the session's player is joining the server with this hash, which the
// server then confirms with the session service.
func (s Services) Join(ctx context.Context, session Session, serverHash string) error {
	body, err := json.Marshal(map[string]string{
		"accessToken":     session.AccessToken,
		"selectedProfile": strings.ReplaceAll(session.Profile.ID, "-", ""),
		"serverId":        serverHash,
	})
	if err != nil {
		return err
	}
	return s.call(ctx, "session join", http.MethodPost, orDefault(s.JoinURL, defaultJoinURL), "", body, nil)
}

// call performs one request and decodes a JSON response into out when out is not nil. The
// response body never appears in an error: it can carry tokens.
func (s Services) call(ctx context.Context, operation, method, url, bearer string, body []byte, out any) error {
	request, err := http.NewRequestWithContext(ctx, method, url, bytes.NewReader(body))
	if err != nil {
		return err
	}
	if body != nil {
		request.Header.Set("Content-Type", "application/json")
	}
	request.Header.Set("Accept", "application/json")
	if bearer != "" {
		request.Header.Set("Authorization", "Bearer "+bearer)
	}
	client := s.HTTP
	if client == nil {
		client = http.DefaultClient
	}
	response, err := client.Do(request)
	if err != nil {
		return fmt.Errorf("java edition %s: %w", operation, err)
	}
	defer response.Body.Close()
	data, err := io.ReadAll(io.LimitReader(response.Body, maxResponseBytes))
	if err != nil {
		return fmt.Errorf("java edition %s: %w", operation, err)
	}
	if response.StatusCode < 200 || response.StatusCode > 299 {
		return &StatusError{Operation: operation, Status: response.StatusCode}
	}
	if out == nil || len(data) == 0 {
		return nil
	}
	if err := json.Unmarshal(data, out); err != nil {
		return fmt.Errorf("java edition %s: malformed response", operation)
	}
	return nil
}

// orDefault returns value, or fallback when value is empty.
func orDefault(value, fallback string) string {
	if value == "" {
		return fallback
	}
	return value
}
