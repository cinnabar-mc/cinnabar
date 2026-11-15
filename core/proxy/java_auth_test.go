package proxy

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/javaedition/auth"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/javatest"
	"github.com/sandertv/gophertunnel/minecraft"
	"golang.org/x/oauth2"
)

// switchStub is a Microsoft app whose device-code sign-in never finishes and whose identity
// token is fixed.
type switchStub struct{}

func (switchStub) DeviceAuth(context.Context) (*oauth2.DeviceAuthResponse, error) {
	return &oauth2.DeviceAuthResponse{UserCode: "TESTCODE", VerificationURI: "https://www.microsoft.com/link", Expiry: time.Now().Add(time.Minute)}, nil
}

func (switchStub) DeviceAccessToken(ctx context.Context, _ *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
	<-ctx.Done()
	return nil, ctx.Err()
}

func (switchStub) TokenSource(_ context.Context, token *oauth2.Token) oauth2.TokenSource {
	return oauth2.StaticTokenSource(token)
}

func (switchStub) IdentityToken(context.Context, oauth2.TokenSource) (string, error) {
	return "XBL3.0 x=hash;switch", nil
}

// javaServices fakes the Java Edition services and records the server hash a join names.
type javaServices struct {
	mu   sync.Mutex
	hash string
}

func (fake *javaServices) start(t *testing.T) auth.Services {
	t.Helper()
	mux := http.NewServeMux()
	mux.HandleFunc("/login", func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"access_token": "java-access", "expires_in": 86400})
	})
	mux.HandleFunc("/profile", func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(auth.Profile{ID: "069a79f444e94726a5befca90e38aaf5", Name: "Notch"})
	})
	mux.HandleFunc("/join", func(w http.ResponseWriter, r *http.Request) {
		var body map[string]string
		_ = json.NewDecoder(r.Body).Decode(&body)
		fake.mu.Lock()
		fake.hash = body["serverId"]
		fake.mu.Unlock()
		w.WriteHeader(http.StatusNoContent)
	})
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)
	return auth.Services{HTTP: server.Client(), LoginURL: server.URL + "/login", ProfileURL: server.URL + "/profile", JoinURL: server.URL + "/join"}
}

func startJavaServer(t *testing.T, config javatest.Config) *javatest.Server {
	t.Helper()
	server, err := javatest.Start(config)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(server.Close)
	return server
}

// disconnectText returns the disconnect message a failed join shows the player.
func disconnectText(t *testing.T, err error) string {
	t.Helper()
	var disconnect *minecraft.DisconnectPacketError
	if !errors.As(err, &disconnect) {
		t.Fatalf("join failed without disconnect text: %v", err)
	}
	return disconnect.Message
}

func dialJavaForTest(t *testing.T, server *javatest.Server, accounts *auth.Accounts) (upstreamSession, error) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	return dialJavaUpstream(ctx, server.Addr(), accounts, "Steve", slog.New(slog.DiscardHandler))
}

const notchID = "069a79f444e94726a5befca90e38aaf5"

// testAccounts opens a Java accounts directory holding one selected account named name,
// whose token is token.
func testAccounts(t *testing.T, services auth.Services, name string, token []byte) *auth.Accounts {
	t.Helper()
	dir := t.TempDir()
	index, err := json.Marshal(map[string]any{"selected": notchID, "accounts": []auth.Account{{ID: notchID, Name: name}}})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "accounts.json"), index, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, notchID+".json"), token, 0o600); err != nil {
		t.Fatal(err)
	}
	accounts := auth.OpenAccounts(auth.AccountsConfig{Dir: dir, App: switchStub{}, Services: services})
	t.Cleanup(accounts.Close)
	return accounts
}

func workingToken(t *testing.T) []byte {
	t.Helper()
	token, err := json.Marshal(&oauth2.Token{AccessToken: "switch-access", RefreshToken: "switch-refresh", Expiry: time.Now().Add(time.Hour)})
	if err != nil {
		t.Fatal(err)
	}
	return token
}

// An online-mode server asks for the session of a player with no Java account: the join
// starts adding one and fails with its code, which the Accounts screen shows too.
func TestJavaJoinWithoutAnAccountShowsTheCode(t *testing.T) {
	server := startJavaServer(t, javatest.Config{OnlineMode: true})
	accounts := auth.OpenAccounts(auth.AccountsConfig{Dir: t.TempDir(), App: switchStub{}})
	t.Cleanup(accounts.Close)
	_, err := dialJavaForTest(t, server, accounts)
	text := disconnectText(t, err)
	if !strings.Contains(text, "TESTCODE") || !strings.Contains(text, "https://www.microsoft.com/link") {
		t.Fatalf("disconnect text = %q", text)
	}
	if add := accounts.AddState(); add.UserCode != "TESTCODE" {
		t.Fatalf("add state = %+v", add)
	}
}

// Without Java accounts at all, an online-mode server cannot be joined, and the player is
// told where to add one.
func TestJavaJoinWithoutAccountsAsksForOne(t *testing.T) {
	server := startJavaServer(t, javatest.Config{OnlineMode: true})
	_, err := dialJavaForTest(t, server, nil)
	if text := disconnectText(t, err); !strings.Contains(text, "Add one in Accounts") {
		t.Fatalf("disconnect text = %q", text)
	}
}

// The selected account logs in as its saved profile and registers the join with the session
// service.
func TestJavaJoinUsesTheSelectedAccount(t *testing.T) {
	server := startJavaServer(t, javatest.Config{OnlineMode: true})
	fake := &javaServices{}
	accounts := testAccounts(t, fake.start(t), "Notch", workingToken(t))
	joined := make(chan *javatest.Client, 1)
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		client, _ := server.Accept(ctx)
		joined <- client
	}()
	upstream, err := dialJavaForTest(t, server, accounts)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = upstream.Close() })
	client := <-joined
	if client == nil || client.Username != "Notch" {
		t.Fatalf("the server saw %+v", client)
	}
	fake.mu.Lock()
	defer fake.mu.Unlock()
	if fake.hash == "" {
		t.Fatal("the join was not registered with the session service")
	}
}

// A selected account whose sign-in no longer works asks for a new one, as no account does.
func TestJavaJoinWithAnUnusableSignInShowsTheCode(t *testing.T) {
	server := startJavaServer(t, javatest.Config{OnlineMode: true})
	accounts := testAccounts(t, (&javaServices{}).start(t), "Notch", []byte("not a token"))
	_, err := dialJavaForTest(t, server, accounts)
	if text := disconnectText(t, err); !strings.Contains(text, "TESTCODE") {
		t.Fatalf("disconnect text = %q", text)
	}
}

// A profile renamed since it was saved logged in under its old name, which the server will
// not accept: the player is asked to reconnect, and the next login sends the new name.
func TestJavaJoinAfterARenameAsksToReconnect(t *testing.T) {
	server := startJavaServer(t, javatest.Config{OnlineMode: true})
	accounts := testAccounts(t, (&javaServices{}).start(t), "OldName", workingToken(t))
	_, err := dialJavaForTest(t, server, accounts)
	if text := disconnectText(t, err); !strings.Contains(text, "Reconnect") {
		t.Fatalf("disconnect text = %q", text)
	}
	if account, _, ok := accounts.Selected(); !ok || account.Name != "Notch" {
		t.Fatalf("selected after the rename = %+v", account)
	}
}

// A server that refuses the login shows the player its own reason.
func TestJavaJoinShowsTheServersRefusal(t *testing.T) {
	server := startJavaServer(t, javatest.Config{RefuseLogin: `{"text":"You are banned","color":"red"}`})
	_, err := dialJavaForTest(t, server, nil)
	if text := disconnectText(t, err); !strings.Contains(text, "You are banned") {
		t.Fatalf("disconnect text = %q", text)
	}
}

// A sign-in Microsoft did not refresh and an account changed during the join each tell the
// player to reconnect, rather than to add an account.
func TestJavaJoinErrorAsksToReconnect(t *testing.T) {
	for _, err := range []error{
		fmt.Errorf("%w: dial tcp: connection refused", auth.ErrServicesUnreachable),
		errJavaAccountChanged,
	} {
		if text := disconnectText(t, javaJoinError(err)); !strings.Contains(text, "Reconnect") {
			t.Errorf("disconnect text for %v = %q", err, text)
		}
	}
}
