package authflow

import (
	"bytes"
	"context"
	"crypto/tls"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
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
		return nil, &deviceFailure{reason: "device_start:" + failureReason(err)}
	}
	if err := validatePrompt(response); err != nil {
		return nil, &deviceFailure{reason: "device_prompt"}
	}
	if err := publish(response); err != nil {
		return nil, &deviceFailure{reason: "device_publish"}
	}
	token, err := f.Token(tolerantPollContext(ctx), response)
	if err != nil {
		if deviceCodeExpired(ctx, response, err) {
			return nil, fmt.Errorf("%w: %w", &deviceFailure{reason: "device_expired"}, errDeviceCodeExpired)
		}
		return nil, &deviceFailure{reason: "device_complete:" + failureReason(err)}
	}
	return token, nil
}

// deviceFailure is a failed device flow reduced to a credential-free reason for the log.
type deviceFailure struct {
	reason string
}

func (e *deviceFailure) Error() string { return errDeviceAuthorization.Error() + ": " + e.reason }

func (e *deviceFailure) Is(target error) bool { return target == errDeviceAuthorization }

// standardErrorCodes are the OAuth token errors of RFC 6749 section 5.2, RFC 8628 section 3.5 and
// OpenID Connect that may be logged; any other provider-controlled value is reduced to a marker.
var standardErrorCodes = map[string]struct{}{
	"access_denied": {}, "account_selection_required": {}, "authorization_pending": {},
	"consent_required": {}, "expired_token": {}, "interaction_required": {}, "invalid_client": {},
	"invalid_grant": {}, "invalid_request": {}, "invalid_scope": {}, "login_required": {},
	"server_error": {}, "slow_down": {}, "temporarily_unavailable": {}, "unauthorized_client": {},
	"unsupported_grant_type": {},
}

// failureReason classifies a device-flow request failure without keeping any provider text:
// only the HTTP status, a standard OAuth error code and the kind of connection failure survive.
func failureReason(err error) string {
	var provider *oauth2.RetrieveError
	var certificate *tls.CertificateVerificationError
	var timeout interface{ Timeout() bool }
	var request *url.Error
	switch {
	case errors.As(err, &provider):
		return providerReason(provider)
	case errors.Is(err, context.Canceled):
		return "cancelled"
	case errors.As(err, &certificate):
		return "tls"
	case errors.Is(err, context.DeadlineExceeded), errors.As(err, &timeout) && timeout.Timeout():
		return "timeout"
	case errors.As(err, &request):
		return "transport"
	default:
		return "invalid_response"
	}
}

// providerReason names a provider's answer by its status and, when standard, its OAuth error code.
func providerReason(provider *oauth2.RetrieveError) string {
	reason := "http_unknown"
	if provider.Response != nil {
		reason = fmt.Sprintf("http_%d", provider.Response.StatusCode)
	}
	if provider.ErrorCode == "" {
		return reason
	}
	if _, standard := standardErrorCodes[provider.ErrorCode]; standard {
		return reason + ":" + provider.ErrorCode
	}
	return reason + ":unrecognized_code"
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
