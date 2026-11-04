package authflow

import (
	"bytes"
	"context"
	"crypto/tls"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"golang.org/x/oauth2"
)

func TestCacheFailureDiagnosticSurvivesSignInAndRetryWithoutLeakingCredentials(t *testing.T) {
	for _, test := range []struct {
		name       string
		diagnostic string
		setup      func(*testing.T, string)
	}{
		{"cache is a directory", "cache_load", func(t *testing.T, path string) {
			if err := os.Mkdir(path, 0o700); err != nil {
				t.Fatal(err)
			}
		}},
		{"lock is a directory", "cache_lock:os_", func(t *testing.T, path string) {
			if err := os.Mkdir(path+".lock", 0o700); err != nil {
				t.Fatal(err)
			}
		}},
	} {
		t.Run(test.name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "private-path-sentinel.json")
			test.setup(t, path)
			for range 2 {
				var output, logs bytes.Buffer
				err := Run(context.Background(), Config{
					Path: path, Writer: &output, Logger: slog.New(slog.NewTextHandler(&logs, nil)),
					DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
						t.Fatal("cache failure unexpectedly requested device authentication")
						return nil, nil
					},
				})
				if err == nil {
					t.Fatal("invalid cache succeeded")
				}
				events := decodeEvents(t, output.Bytes())
				if got := events[len(events)-1]; got.Stage != "cache" || got.Message != "Could not validate the saved account." {
					t.Fatalf("terminal event = %#v", got)
				}
				if !strings.Contains(logs.String(), "diagnostic="+test.diagnostic) {
					t.Fatalf("missing cache diagnostic: %s", logs.String())
				}
				combined := output.String() + logs.String() + err.Error()
				for _, secret := range []string{"credential-sentinel", "private-path-sentinel"} {
					if strings.Contains(combined, secret) {
						t.Fatalf("failure exposed %s", secret)
					}
				}
			}
		})
	}
}

func TestUnknownSignInFailureIsNotFormattedInDiagnostics(t *testing.T) {
	var logs bytes.Buffer
	err := Run(context.Background(), Config{
		Path: "unused", Logger: slog.New(slog.NewTextHandler(&logs, &slog.HandlerOptions{Level: slog.LevelError})),
		CachedSource: func(context.Context, authcache.Config) (oauth2.TokenSource, error) {
			return nil, errors.New("provider-credential-sentinel")
		},
	})
	if err == nil || !strings.Contains(logs.String(), "diagnostic=unclassified") || strings.Contains(logs.String(), "provider-credential-sentinel") {
		t.Fatalf("unsafe diagnostic: %s", logs.String())
	}
}

// runDeviceFailure signs in through Run and returns its log with the device flow's failing step.
func runDeviceFailure(t *testing.T, authorize func(context.Context) (*oauth2.DeviceAuthResponse, error), token func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error)) string {
	t.Helper()
	var output, logs bytes.Buffer
	err := Run(context.Background(), Config{
		Path: filepath.Join(t.TempDir(), "private-path-sentinel.json"), Writer: &output,
		Logger:     slog.New(slog.NewTextHandler(&logs, nil)),
		DeviceAuth: authorize, DeviceToken: token,
	})
	if err == nil {
		t.Fatal("failing device flow succeeded")
	}
	events := decodeEvents(t, output.Bytes())
	if last := events[len(events)-1]; last.Stage != "device_code" || last.Message != "Microsoft sign-in did not complete. Try again." {
		t.Fatalf("terminal event = %#v", last)
	}
	combined := output.String() + logs.String() + err.Error()
	for _, secret := range []string{"provider-secret", "provider_secret", "private-path-sentinel"} {
		if strings.Contains(combined, secret) {
			t.Fatalf("failure exposed %s: %s", secret, combined)
		}
	}
	return logs.String()
}

func TestDeviceFailureDiagnosticNamesTheStepAndProviderAnswer(t *testing.T) {
	prompt := func(context.Context) (*oauth2.DeviceAuthResponse, error) {
		return &oauth2.DeviceAuthResponse{VerificationURI: "https://login.example.test/device", UserCode: "ABCD-1234"}, nil
	}
	rejectedPoll := func(failure error) func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
		return func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) { return nil, failure }
	}
	answer := func(status int, code string) *oauth2.RetrieveError {
		return &oauth2.RetrieveError{
			Response:  &http.Response{StatusCode: status},
			ErrorCode: code, ErrorDescription: "provider-secret", Body: []byte("provider-secret"),
		}
	}
	unknownCA := &url.Error{Op: "Post", URL: "https://provider-secret.example.test", Err: &tls.CertificateVerificationError{Err: errors.New("provider-secret")}}
	for _, test := range []struct {
		name, want string
		authorize  func(context.Context) (*oauth2.DeviceAuthResponse, error)
		token      func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error)
	}{
		{"provider rejects the code", "device_complete:http_400:invalid_grant", prompt, rejectedPoll(answer(http.StatusBadRequest, "invalid_grant"))},
		{"provider sends an unreadable answer", "device_complete:http_403", prompt, rejectedPoll(answer(http.StatusForbidden, ""))},
		{"provider code is not a standard one", "device_complete:http_400:unrecognized_code", prompt, rejectedPoll(answer(http.StatusBadRequest, "provider_secret"))},
		{"accepted answer has no token", "device_complete:invalid_response", prompt, rejectedPoll(errors.New("oauth2: server response missing access_token provider-secret"))},
		{"prompt request cannot connect", "device_start:transport", func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return nil, &url.Error{Op: "Post", URL: "https://provider-secret.example.test", Err: syscall.ECONNRESET}
		}, nil},
		{"prompt request times out", "device_start:timeout", func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return nil, &url.Error{Op: "Post", URL: "https://provider-secret.example.test", Err: context.DeadlineExceeded}
		}, nil},
		{"prompt certificate is not trusted", "device_start:tls", func(context.Context) (*oauth2.DeviceAuthResponse, error) { return nil, unknownCA }, nil},
		{"prompt is unsafe", "device_prompt", func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return &oauth2.DeviceAuthResponse{VerificationURI: "http://provider-secret.example.test", UserCode: "ABCD-1234"}, nil
		}, nil},
	} {
		t.Run(test.name, func(t *testing.T) {
			logs := runDeviceFailure(t, test.authorize, test.token)
			if !strings.Contains(logs, "diagnostic="+test.want+"\n") {
				t.Fatalf("missing diagnostic %s: %s", test.want, logs)
			}
		})
	}
}

func TestSignInLogsEachStepBeforeItWaits(t *testing.T) {
	var logs bytes.Buffer
	path := filepath.Join(t.TempDir(), "token.json")
	logged := func() string { return logs.String() }
	err := Run(context.Background(), Config{
		Path: path, Writer: io.Discard, Logger: slog.New(slog.NewTextHandler(&logs, nil)),
		DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			if !strings.Contains(logged(), "sign-in requesting device code") {
				t.Errorf("no step logged before the prompt request: %s", logged())
			}
			return &oauth2.DeviceAuthResponse{VerificationURI: "https://login.example.test/device", UserCode: "ABCD-1234"}, nil
		},
		DeviceToken: func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
			if !strings.Contains(logged(), "sign-in waiting for browser approval") {
				t.Errorf("no step logged before polling: %s", logged())
			}
			return &oauth2.Token{AccessToken: "provider-secret", RefreshToken: "provider-secret", Expiry: time.Now().Add(time.Hour)}, nil
		},
		Refresh: func(token *oauth2.Token, _ io.Writer) oauth2.TokenSource { return oauth2.StaticTokenSource(token) },
	})
	if err != nil {
		t.Fatal(err)
	}
	for _, step := range []string{"sign-in checking saved account", "sign-in approved in browser", "sign-in token ready"} {
		if !strings.Contains(logged(), step) {
			t.Fatalf("missing step %q: %s", step, logged())
		}
	}
	if strings.Contains(logged(), "provider-secret") || strings.Contains(logged(), "ABCD-1234") {
		t.Fatalf("step log exposed account material: %s", logged())
	}
}
