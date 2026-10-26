package authflow

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"time"

	"github.com/sandertv/gophertunnel/minecraft/auth"
	"golang.org/x/oauth2"
)

// DeviceFlow runs Microsoft device authentication. Unset operations use the Android client.
type DeviceFlow struct {
	Authorize func(context.Context) (*oauth2.DeviceAuthResponse, error)
	Token     func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error)
}

// Request validates and publishes a device prompt before polling for its token.
// Provider error details are discarded because they may contain credentials.
func (f DeviceFlow) Request(ctx context.Context, publish func(*oauth2.DeviceAuthResponse) error) (*oauth2.Token, error) {
	if f.Authorize == nil {
		f.Authorize = auth.AndroidConfig.DeviceAuth
	}
	if f.Token == nil {
		f.Token = auth.AndroidConfig.DeviceAccessToken
	}
	response, err := f.Authorize(ctx)
	if err != nil {
		return nil, fmt.Errorf("%w: start", errDeviceAuthorization)
	}
	if err := validatePrompt(response); err != nil {
		return nil, fmt.Errorf("%w: invalid prompt", errDeviceAuthorization)
	}
	if err := publish(response); err != nil {
		return nil, fmt.Errorf("%w: publish prompt", errDeviceAuthorization)
	}
	token, err := f.Token(tolerantPollContext(ctx), response)
	if err != nil {
		if deviceCodeExpired(ctx, response, err) {
			return nil, fmt.Errorf("%w: %w", errDeviceAuthorization, errDeviceCodeExpired)
		}
		return nil, fmt.Errorf("%w: complete", errDeviceAuthorization)
	}
	return token, nil
}

// deviceCodeExpired distinguishes device expiry from unrelated caller timeouts without retaining provider details.
func deviceCodeExpired(ctx context.Context, response *oauth2.DeviceAuthResponse, err error) bool {
	var providerError *oauth2.RetrieveError
	if errors.As(err, &providerError) && providerError.ErrorCode == "expired_token" {
		return true
	}
	if !errors.Is(err, context.DeadlineExceeded) || response.Expiry.IsZero() || time.Now().Before(response.Expiry) {
		return false
	}
	deadline, hasDeadline := ctx.Deadline()
	return !hasDeadline || !deadline.Before(response.Expiry)
}

// maxPollResponseBytes matches the token response size oauth2 reads.
const maxPollResponseBytes = 1 << 20

// pendingPollResponse is the device-flow answer that tells oauth2 to keep polling.
var pendingPollResponse = []byte(`{"error":"authorization_pending"}`)

// tolerantPollContext returns ctx with an OAuth HTTP client whose token polls survive failures
// that leave the device code unanswered. Mobile systems suspend the app while the player signs in
// through the browser, breaking or timing out in-flight polls. Reporting those as pending keeps
// oauth2's own polling loop, so provider slow_down backoff, cancellation and device-code expiry
// still end the flow exactly as before.
func tolerantPollContext(ctx context.Context) context.Context {
	client, _ := ctx.Value(oauth2.HTTPClient).(*http.Client)
	if client == nil {
		client = http.DefaultClient
	}
	tolerant := *client
	tolerant.Transport = pollTransport{base: client.Transport}
	return context.WithValue(ctx, oauth2.HTTPClient, &tolerant)
}

// pollTransport answers a token poll as pending when the request fails, its response body cannot
// be read, or the provider reports a server error. Provider answers below 500 pass through intact.
type pollTransport struct {
	base http.RoundTripper
}

func (t pollTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	base := t.base
	if base == nil {
		base = http.DefaultTransport
	}
	response, err := base.RoundTrip(request)
	if err != nil {
		return pendingPoll(request), nil
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, maxPollResponseBytes))
	_ = response.Body.Close()
	if err != nil || response.StatusCode >= http.StatusInternalServerError {
		return pendingPoll(request), nil
	}
	response.Body = io.NopCloser(bytes.NewReader(body))
	response.ContentLength = int64(len(body))
	return response, nil
}

// pendingPoll builds an authorization_pending token response for request.
func pendingPoll(request *http.Request) *http.Response {
	return &http.Response{
		Status: "400 Bad Request", StatusCode: http.StatusBadRequest,
		Proto: "HTTP/1.1", ProtoMajor: 1, ProtoMinor: 1,
		Header:        http.Header{"Content-Type": {"application/json"}},
		Body:          io.NopCloser(bytes.NewReader(pendingPollResponse)),
		ContentLength: int64(len(pendingPollResponse)),
		Request:       request,
	}
}
