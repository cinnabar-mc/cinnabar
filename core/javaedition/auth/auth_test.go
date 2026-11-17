package auth

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

// fakeServices serves the three endpoints and records what each received.
type fakeServices struct {
	server        *httptest.Server
	identityToken string
	bearer        string
	join          map[string]string
	profileStatus int
	joinStatus    int
}

func newFakeServices(t *testing.T) *fakeServices {
	t.Helper()
	fake := &fakeServices{profileStatus: http.StatusOK, joinStatus: http.StatusNoContent}
	mux := http.NewServeMux()
	mux.HandleFunc("/login", func(w http.ResponseWriter, r *http.Request) {
		var body map[string]string
		_ = json.NewDecoder(r.Body).Decode(&body)
		fake.identityToken = body["identityToken"]
		_ = json.NewEncoder(w).Encode(map[string]any{"access_token": "secret-token", "expires_in": 86400})
	})
	mux.HandleFunc("/profile", func(w http.ResponseWriter, r *http.Request) {
		fake.bearer = r.Header.Get("Authorization")
		if fake.profileStatus != http.StatusOK {
			w.WriteHeader(fake.profileStatus)
			return
		}
		_ = json.NewEncoder(w).Encode(Profile{ID: "069a79f444e94726a5befca90e38aaf5", Name: "Notch"})
	})
	mux.HandleFunc("/join", func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewDecoder(r.Body).Decode(&fake.join)
		w.WriteHeader(fake.joinStatus)
	})
	fake.server = httptest.NewServer(mux)
	t.Cleanup(fake.server.Close)
	return fake
}

func (fake *fakeServices) services() Services {
	return Services{
		HTTP:       fake.server.Client(),
		LoginURL:   fake.server.URL + "/login",
		ProfileURL: fake.server.URL + "/profile",
		JoinURL:    fake.server.URL + "/join",
	}
}

func TestSignInAndJoin(t *testing.T) {
	fake := newFakeServices(t)
	services := fake.services()
	session, err := services.SignIn(context.Background(), "XBL3.0 x=hash;token")
	if err != nil {
		t.Fatal(err)
	}
	if fake.identityToken != "XBL3.0 x=hash;token" || fake.bearer != "Bearer secret-token" {
		t.Fatalf("services saw identity %q and bearer %q", fake.identityToken, fake.bearer)
	}
	if session.Profile.Name != "Notch" || session.AccessToken != "secret-token" {
		t.Fatalf("session = %+v", session)
	}
	session.Profile.ID = "069a79f4-44e9-4726-a5be-fca90e38aaf5"
	if err := services.Join(context.Background(), session, "-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1"); err != nil {
		t.Fatal(err)
	}
	want := map[string]string{
		"accessToken":     "secret-token",
		"selectedProfile": "069a79f444e94726a5befca90e38aaf5",
		"serverId":        "-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1",
	}
	for key, value := range want {
		if fake.join[key] != value {
			t.Errorf("join %s = %q, want %q", key, fake.join[key], value)
		}
	}
}

func TestSignInWithoutProfile(t *testing.T) {
	fake := newFakeServices(t)
	fake.profileStatus = http.StatusNotFound
	if _, err := fake.services().SignIn(context.Background(), "token"); !errors.Is(err, ErrNoProfile) {
		t.Fatalf("err = %v, want ErrNoProfile", err)
	}
}

func TestJoinRefused(t *testing.T) {
	fake := newFakeServices(t)
	fake.joinStatus = http.StatusForbidden
	err := fake.services().Join(context.Background(), Session{AccessToken: "secret-token"}, "hash")
	var status *StatusError
	if !errors.As(err, &status) || status.Status != http.StatusForbidden {
		t.Fatalf("err = %v, want HTTP 403", err)
	}
	if got := err.Error(); got != "java edition session join: HTTP 403" {
		t.Fatalf("error text %q", got)
	}
}

// A refused sign-in keeps the service's own explanation, which names the cause (such as an
// app the services do not accept), and nothing else from the response.
func TestSignInFailureCarriesTheServiceMessage(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusForbidden)
		_ = json.NewEncoder(w).Encode(map[string]string{
			"path":         "/authentication/login_with_xbox",
			"errorMessage": "Invalid app registration, see https://aka.ms/AppRegInfo for more information",
			"access_token": "must-not-leak",
		})
	}))
	t.Cleanup(server.Close)
	_, err := Services{HTTP: server.Client(), LoginURL: server.URL}.SignIn(context.Background(), "XBL3.0 x=hash;token")
	var status *StatusError
	if !errors.As(err, &status) || status.Status != http.StatusForbidden {
		t.Fatalf("sign-in error = %v", err)
	}
	if !strings.HasPrefix(status.Message, "Invalid app registration") || strings.Contains(err.Error(), "must-not-leak") {
		t.Fatalf("sign-in error = %q", err)
	}
}

// The session's expiry comes from the service's expires_in.
func TestSignInRecordsTheSessionExpiry(t *testing.T) {
	fake := newFakeServices(t)
	before := time.Now()
	session, err := fake.services().SignIn(context.Background(), "XBL3.0 x=hash;token")
	if err != nil {
		t.Fatal(err)
	}
	if lifetime := session.Expires.Sub(before); lifetime < 24*time.Hour-time.Minute || lifetime > 24*time.Hour+time.Minute {
		t.Fatalf("session lasts %v", lifetime)
	}
}
