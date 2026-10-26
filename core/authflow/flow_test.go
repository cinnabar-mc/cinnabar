package authflow

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"testing"
	"testing/synctest"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"golang.org/x/oauth2"
)

func TestFirstTimeSequencePersistsWithoutLeakingTokens(t *testing.T) {
	const access = "access-token-sentinel"
	const refresh = "refresh-token-sentinel"
	path := filepath.Join(t.TempDir(), "auth", "token.json")
	var output bytes.Buffer
	err := Run(context.Background(), Config{
		Path: path, Writer: &output,
		DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return &oauth2.DeviceAuthResponse{VerificationURI: "https://login.example.test/device", UserCode: "ABCD-1234"}, nil
		},
		DeviceToken: func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
			return validToken(access, refresh), nil
		},
		Refresh: staticRefresh,
	})
	if err != nil {
		t.Fatalf("Run() error = %v", err)
	}
	wantKinds := []string{"checking_cache", "device_code", "authenticated"}
	events := decodeEvents(t, output.Bytes())
	for index, want := range wantKinds {
		if events[index].Kind != want {
			t.Fatalf("event %d = %q, want %q", index, events[index].Kind, want)
		}
	}
	if events[2].Method != "device_code" {
		t.Fatalf("authenticated method = %q", events[2].Method)
	}
	if strings.Contains(output.String(), access) || strings.Contains(output.String(), refresh) || strings.Contains(errString(err), access) {
		t.Fatal("authentication event stream leaked token material")
	}
	contents, readErr := os.ReadFile(path)
	if readErr != nil || !bytes.Contains(contents, []byte(refresh)) {
		t.Fatalf("persisted cache missing expected injected token: err=%v", readErr)
	}
}

func TestCachedSequenceDoesNotRequestDeviceCode(t *testing.T) {
	path := filepath.Join(t.TempDir(), "token.json")
	// Seed the cache through the real persistence path so the fixture matches
	// what this program actually publishes, including its private-file
	// contract.
	_, err := authcache.Source(context.Background(), authcache.Config{
		Path: path,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			return validToken("cached-access", "cached-refresh"), nil
		},
		Refresh: staticRefresh,
	})
	if err != nil {
		t.Fatal(err)
	}
	var output bytes.Buffer
	err = Run(context.Background(), Config{
		Path: path, Writer: &output,
		DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			t.Fatal("cached flow requested a device code")
			return nil, nil
		},
		Refresh: staticRefresh,
	})
	if err != nil {
		t.Fatal(err)
	}
	events := decodeEvents(t, output.Bytes())
	if len(events) != 2 || events[1].Kind != "authenticated" || events[1].Method != "cached" {
		t.Fatalf("events = %#v", events)
	}
}

func TestFailureIsStagedAndUnderlyingSecretIsNotPublished(t *testing.T) {
	const sentinel = "provider-secret-sentinel"
	var output bytes.Buffer
	err := Run(context.Background(), Config{
		Path: filepath.Join(t.TempDir(), "token.json"), Writer: &output,
		DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return nil, errors.New(sentinel)
		},
	})
	if err == nil {
		t.Fatal("Run() succeeded")
	}
	if strings.Contains(output.String(), sentinel) || strings.Contains(err.Error(), sentinel) {
		t.Fatal("provider error detail leaked")
	}
	events := decodeEvents(t, output.Bytes())
	if got := events[len(events)-1]; got.Kind != "error" || got.Stage != "device_code" {
		t.Fatalf("terminal event = %#v", got)
	}
}

func TestPromptValidationRejectsUnsafeValues(t *testing.T) {
	cases := []oauth2.DeviceAuthResponse{
		{VerificationURI: "http://example.test", UserCode: "GOOD"},
		{VerificationURI: "https://user@example.test", UserCode: "GOOD"},
		{VerificationURI: "https://example.test", UserCode: "bad\ncode"},
		{VerificationURI: "https://example.test", UserCode: strings.Repeat("A", 65)},
	}
	for _, response := range cases {
		if err := validatePrompt(&response); err == nil {
			t.Fatalf("validatePrompt(%#v) succeeded", response)
		}
	}
}

func TestCancellationEmitsSafeTerminalEvent(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	var output bytes.Buffer
	err := Run(ctx, Config{
		Path: filepath.Join(t.TempDir(), "token.json"), Writer: &output,
		DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return nil, context.Canceled
		},
	})
	if err == nil {
		t.Fatal("Run() succeeded")
	}
	events := decodeEvents(t, output.Bytes())
	if got := events[len(events)-1]; got.Stage != "cancelled" {
		t.Fatalf("terminal stage = %q", got.Stage)
	}
}

func TestConfiguredSourceCannotSmuggleTokenIntoEvents(t *testing.T) {
	const sentinel = "token-sentinel Microsoft token device authorization"
	var output bytes.Buffer
	err := Run(context.Background(), Config{
		Path: "ignored", Writer: &output,
		CachedSource: func(context.Context, authcache.Config) (oauth2.TokenSource, error) {
			return nil, errors.New(sentinel)
		},
	})
	if err == nil || strings.Contains(output.String(), sentinel) || strings.Contains(err.Error(), sentinel) {
		t.Fatalf("unsafe result: output=%q err=%v", output.String(), err)
	}
	events := decodeEvents(t, output.Bytes())
	if got := events[len(events)-1]; got.Stage != "cache" {
		t.Fatalf("terminal stage = %q, want cache", got.Stage)
	}
}

func validToken(access, refresh string) *oauth2.Token {
	return &oauth2.Token{AccessToken: access, RefreshToken: refresh, Expiry: time.Now().Add(time.Hour)}
}

func staticRefresh(token *oauth2.Token, _ io.Writer) oauth2.TokenSource {
	return oauth2.StaticTokenSource(token)
}

func decodeEvents(t *testing.T, data []byte) []event {
	t.Helper()
	var events []event
	decoder := json.NewDecoder(bytes.NewReader(data))
	for decoder.More() {
		var value event
		if err := decoder.Decode(&value); err != nil {
			t.Fatal(err)
		}
		events = append(events, value)
	}
	return events
}

func errString(err error) string {
	if err == nil {
		return ""
	}
	return err.Error()
}

func TestDeviceFlowNeverPollsAnUnsafeOrUnpublishedPrompt(t *testing.T) {
	for _, unsafe := range []bool{false, true} {
		flow := DeviceFlow{
			Authorize: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
				response := &oauth2.DeviceAuthResponse{VerificationURI: "https://login.example.test", UserCode: "SAFE"}
				if unsafe {
					response.UserCode = "bad\ncode"
				}
				return response, nil
			},
			Token: func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
				t.Fatal("polled before a valid prompt was published")
				return nil, nil
			},
		}
		_, err := flow.Request(context.Background(), func(*oauth2.DeviceAuthResponse) error {
			if unsafe {
				t.Fatal("published an unsafe prompt")
			}
			return errors.New("writer failed with secret provider data")
		})
		if !errors.Is(err, errDeviceAuthorization) || strings.Contains(err.Error(), "secret") {
			t.Fatalf("device flow error = %v", err)
		}
	}
}

// Sign-in completes the join prerequisites before reporting success, and never after a failed sign-in.
func TestSignInCompletesJoinPrerequisitesBeforeReportingSuccess(t *testing.T) {
	path := filepath.Join(t.TempDir(), "auth", "token.json")
	var output bytes.Buffer
	var completed []string
	err := Run(context.Background(), Config{
		Path: path, Writer: &output, Refresh: staticRefresh,
		DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return &oauth2.DeviceAuthResponse{VerificationURI: "https://login.example.test/device", UserCode: "ABCD-1234"}, nil
		},
		DeviceToken: func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
			return validToken("access", "refresh"), nil
		},
		CompleteSignIn: func(_ context.Context, completedPath string, source oauth2.TokenSource) error {
			if token, err := source.Token(); err != nil || token.AccessToken != "access" {
				t.Fatal("sign-in completion did not receive the signed-in source")
			}
			if strings.Contains(output.String(), "authenticated") {
				t.Fatal("success was reported before sign-in completion")
			}
			completed = append(completed, completedPath)
			return errors.New("exchange unavailable")
		},
	})
	if err != nil || len(completed) != 1 || completed[0] != path {
		t.Fatalf("Run err=%v completions=%v", err, completed)
	}
	if events := decodeEvents(t, output.Bytes()); events[len(events)-1].Kind != "authenticated" {
		t.Fatal("a failed exchange blocked sign-in")
	}
	err = Run(context.Background(), Config{
		Path: filepath.Join(t.TempDir(), "token.json"), Writer: io.Discard,
		DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return nil, errors.New("declined")
		},
		CompleteSignIn: func(context.Context, string, oauth2.TokenSource) error {
			t.Fatal("a failed sign-in ran its completion")
			return nil
		},
	})
	if err == nil {
		t.Fatal("declined sign-in succeeded")
	}
}

// Cancelling sign-in while its completion runs reports cancellation, never success.
func TestCancellationDuringSignInCompletionIsNotReportedAsSuccess(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	var output bytes.Buffer
	err := Run(ctx, Config{
		Path: filepath.Join(t.TempDir(), "token.json"), Writer: &output, Refresh: staticRefresh,
		DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return &oauth2.DeviceAuthResponse{VerificationURI: "https://login.example.test/device", UserCode: "ABCD-1234"}, nil
		},
		DeviceToken: func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
			return validToken("access", "refresh"), nil
		},
		CompleteSignIn: func(exchange context.Context, _ string, _ oauth2.TokenSource) error {
			cancel()
			<-exchange.Done()
			return exchange.Err()
		},
	})
	events := decodeEvents(t, output.Bytes())
	if last := events[len(events)-1]; err == nil || last.Kind != "error" || last.Stage != "cancelled" {
		t.Fatalf("Run err=%v last event=%+v, want a cancelled sign-in", err, last)
	}
}

func TestDeviceCodeExpiryReportsSafeRetryMessage(t *testing.T) {
	const secret = "provider-secret-sentinel"
	past := time.Unix(1, 0)
	future := time.Unix(1<<32, 0)
	cases := []struct {
		name       string
		expiry     time.Time
		tokenError error
		deadline   time.Time
		wantExpiry bool
		cancelled  bool
	}{
		{name: "provider expiry", tokenError: &oauth2.RetrieveError{ErrorCode: "expired_token", ErrorDescription: secret}, wantExpiry: true},
		{name: "wrapped provider expiry", tokenError: fmt.Errorf("%s: %w", secret, &oauth2.RetrieveError{ErrorCode: "expired_token", Body: []byte(secret)}), wantExpiry: true},
		{name: "device deadline", expiry: past, tokenError: context.DeadlineExceeded, wantExpiry: true},
		{name: "deadline without expiry", tokenError: context.DeadlineExceeded},
		{name: "deadline before expiry", expiry: future, tokenError: context.DeadlineExceeded},
		{name: "parent deadline before device deadline", expiry: past, deadline: time.Unix(0, 0), tokenError: context.DeadlineExceeded},
		{name: "provider denied", expiry: future, tokenError: &oauth2.RetrieveError{ErrorCode: "access_denied", ErrorDescription: secret}},
		{name: "provider expiry text", tokenError: errors.New("expired_token " + secret)},
		{name: "cancelled expired code", tokenError: &oauth2.RetrieveError{ErrorCode: "expired_token"}, cancelled: true},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			ctx := context.Background()
			if !test.deadline.IsZero() {
				var cancel context.CancelFunc
				ctx, cancel = context.WithDeadline(ctx, test.deadline)
				defer cancel()
			}
			if test.cancelled {
				var cancel context.CancelFunc
				ctx, cancel = context.WithCancel(ctx)
				cancel()
			}
			var output bytes.Buffer
			err := Run(ctx, Config{
				Path: filepath.Join(t.TempDir(), "token.json"), Writer: &output,
				CachedSource: func(ctx context.Context, config authcache.Config) (oauth2.TokenSource, error) {
					_, err := config.Request(ctx, io.Discard)
					return nil, err
				},
				DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
					return &oauth2.DeviceAuthResponse{VerificationURI: "https://login.example.test/device", UserCode: "ABCD-1234", Expiry: test.expiry}, nil
				},
				DeviceToken: func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
					return nil, test.tokenError
				},
			})
			if err == nil {
				t.Fatal("Run() succeeded")
			}
			events := decodeEvents(t, output.Bytes())
			last := events[len(events)-1]
			wantStage := "device_code"
			wantMessage := "Microsoft sign-in did not complete. Try again."
			if test.wantExpiry {
				wantMessage = "Your sign-in code expired. Try again."
			}
			if test.cancelled {
				wantStage, wantMessage = "cancelled", "Sign-in was cancelled."
			}
			if last.Kind != "error" || last.Stage != wantStage || last.Message != wantMessage {
				t.Fatalf("terminal event = %#v, want %s error with %q", last, wantStage, wantMessage)
			}
			if strings.Contains(output.String(), secret) || strings.Contains(err.Error(), secret) {
				t.Fatal("provider error detail leaked")
			}
		})
	}
}

func TestDeviceFlowExpiryDoesNotRetainProviderError(t *testing.T) {
	const secret = "provider-secret-sentinel"
	flow := DeviceFlow{
		Authorize: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return &oauth2.DeviceAuthResponse{VerificationURI: "https://login.example.test/device", UserCode: "ABCD-1234"}, nil
		},
		Token: func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
			return nil, &oauth2.RetrieveError{ErrorCode: "expired_token", ErrorDescription: secret, Body: []byte(secret)}
		},
	}
	_, err := flow.Request(context.Background(), func(*oauth2.DeviceAuthResponse) error { return nil })
	var providerError *oauth2.RetrieveError
	if !errors.Is(err, errDeviceAuthorization) || !errors.Is(err, errDeviceCodeExpired) {
		t.Fatalf("device flow error = %v, want controlled device expiry", err)
	}
	if strings.Contains(err.Error(), secret) || errors.As(err, &providerError) {
		t.Fatal("device flow retained provider error details")
	}
}

// scriptedTokenEndpoint answers successive token polls with steps and records when each arrived.
type scriptedTokenEndpoint struct {
	steps []func(*http.Request) (*http.Response, error)
	polls []time.Time
}

func (e *scriptedTokenEndpoint) RoundTrip(request *http.Request) (*http.Response, error) {
	e.polls = append(e.polls, time.Now())
	step := e.steps[min(len(e.polls), len(e.steps))-1]
	return step(request)
}

// brokenBody fails every read, as a connection dropped mid-response does.
type brokenBody struct{}

func (brokenBody) Read([]byte) (int, error) { return 0, syscall.ECONNRESET }
func (brokenBody) Close() error             { return nil }

func tokenReply(status int, body string) func(*http.Request) (*http.Response, error) {
	return func(request *http.Request) (*http.Response, error) {
		return &http.Response{
			StatusCode: status, Header: http.Header{"Content-Type": {"application/json"}},
			Body: io.NopCloser(strings.NewReader(body)), Request: request,
		}, nil
	}
}

func tokenFailure(err error) func(*http.Request) (*http.Response, error) {
	return func(*http.Request) (*http.Response, error) { return nil, err }
}

// pollDevice runs a device flow whose polls reach endpoint through the real oauth2 poll loop.
func pollDevice(ctx context.Context, endpoint *scriptedTokenEndpoint, expiry time.Time) (*oauth2.Token, error) {
	config := &oauth2.Config{ClientID: "client", Endpoint: oauth2.Endpoint{
		TokenURL: "https://login.example.test/token", AuthStyle: oauth2.AuthStyleInParams,
	}}
	ctx = context.WithValue(ctx, oauth2.HTTPClient, &http.Client{Transport: endpoint})
	flow := DeviceFlow{
		Authorize: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return &oauth2.DeviceAuthResponse{
				DeviceCode: "device", VerificationURI: "https://login.example.test/device", UserCode: "ABCD-1234",
				Expiry: expiry, Interval: 5,
			}, nil
		},
		Token: func(ctx context.Context, response *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
			return config.DeviceAccessToken(ctx, response)
		},
	}
	return flow.Request(ctx, func(*oauth2.DeviceAuthResponse) error { return nil })
}

func TestDeviceFlowKeepsPollingWhileTheAppIsSuspended(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		endpoint := &scriptedTokenEndpoint{steps: []func(*http.Request) (*http.Response, error){
			tokenReply(http.StatusBadRequest, `{"error":"slow_down"}`),
			tokenFailure(&url.Error{Op: "Post", URL: "https://login.example.test/token", Err: syscall.ECONNRESET}),
			tokenFailure(&url.Error{Op: "Post", URL: "https://login.example.test/token", Err: context.DeadlineExceeded}),
			tokenReply(http.StatusServiceUnavailable, `{"error":"temporarily_unavailable"}`),
			func(request *http.Request) (*http.Response, error) {
				return &http.Response{StatusCode: http.StatusOK, Body: brokenBody{}, Request: request}, nil
			},
			tokenReply(http.StatusOK, `{"access_token":"token","token_type":"bearer"}`),
		}}
		start := time.Now()
		token, err := pollDevice(context.Background(), endpoint, start.Add(15*time.Minute))
		if err != nil || token == nil || token.AccessToken != "token" {
			t.Fatalf("Request() = %v, %v; want the token after transient failures", token, err)
		}
		previous := start
		for index, poll := range endpoint.polls {
			want := 10 * time.Second
			if index == 0 {
				want = 5 * time.Second
			}
			if gap := poll.Sub(previous); gap != want {
				t.Fatalf("poll %d came %v after the previous one, want %v (slow_down backoff kept)", index+1, gap, want)
			}
			previous = poll
		}
	})
}

func TestDeviceFlowStillEndsOnDenialAndExpiry(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		denied := &scriptedTokenEndpoint{steps: []func(*http.Request) (*http.Response, error){
			tokenFailure(syscall.ECONNRESET),
			tokenReply(http.StatusBadRequest, `{"error":"access_denied"}`),
		}}
		_, err := pollDevice(context.Background(), denied, time.Now().Add(15*time.Minute))
		if !errors.Is(err, errDeviceAuthorization) || errors.Is(err, errDeviceCodeExpired) || len(denied.polls) != 2 {
			t.Fatalf("denied flow = %v after %d polls, want a failed sign-in after 2", err, len(denied.polls))
		}

		offline := &scriptedTokenEndpoint{steps: []func(*http.Request) (*http.Response, error){tokenFailure(syscall.ENETUNREACH)}}
		_, err = pollDevice(context.Background(), offline, time.Now().Add(time.Minute))
		if !errors.Is(err, errDeviceCodeExpired) {
			t.Fatalf("offline flow = %v, want device-code expiry", err)
		}

		ctx, cancel := context.WithCancel(context.Background())
		cancelled := &scriptedTokenEndpoint{steps: []func(*http.Request) (*http.Response, error){
			func(*http.Request) (*http.Response, error) { cancel(); return nil, context.Canceled },
		}}
		_, err = pollDevice(ctx, cancelled, time.Now().Add(15*time.Minute))
		if err == nil || len(cancelled.polls) != 1 {
			t.Fatalf("cancelled flow = %v after %d polls, want failure after 1", err, len(cancelled.polls))
		}
	})
}
