package authflow

import (
	"bytes"
	"context"
	"crypto/tls"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"mime"
	"net/http"
	"net/url"
	"sync/atomic"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"golang.org/x/oauth2"
)

// DeviceFlow runs Microsoft device authentication. Unset operations use the Android client.
type DeviceFlow struct {
	Authorize func(context.Context) (*oauth2.DeviceAuthResponse, error)
	Token     func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error)
	// Pending is the token cache whose prompt is saved while it awaits approval, so a process
	// ended meanwhile resumes the same code; empty saves nothing. See authcache.SavePendingPrompt.
	Pending string
	// Logger records a prompt that could not be saved; nil discards it.
	Logger *slog.Logger
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
	response, resumed := f.resume()
	if !resumed {
		var err error
		response, err = f.Authorize(ctx)
		if err != nil {
			return nil, &deviceFailure{reason: "device_start:" + failureReason(err)}
		}
		if err := validatePrompt(response); err != nil {
			return nil, &deviceFailure{reason: "device_prompt"}
		}
		f.save(response)
	}
	if err := publish(response); err != nil {
		return nil, &deviceFailure{reason: "device_publish"}
	}
	var lostAnswer atomic.Bool
	token, err := f.Token(tolerantPollContext(ctx, &lostAnswer), response)
	if err == nil || ctx.Err() == nil {
		// The code is settled. A cancelled flow that got no token keeps it, whether the
		// client's process ended or the player cancelled: the next sign-in resumes the
		// unexpired code, which completes at once if the player already approved it.
		f.forget()
	}
	if err != nil {
		if deviceCodeExpired(ctx, response, err) {
			return nil, fmt.Errorf("%w: %w", &deviceFailure{reason: "device_expired"}, errDeviceCodeExpired)
		}
		reason := "device_complete:" + failureReason(err)
		if lostAnswer.Load() {
			reason += ":after_lost_answer"
		}
		return nil, &deviceFailure{reason: reason}
	}
	return token, nil
}

// resume returns the saved prompt still awaiting approval, if Pending names one.
func (f DeviceFlow) resume() (*oauth2.DeviceAuthResponse, bool) {
	if f.Pending == "" {
		return nil, false
	}
	response := authcache.LoadPendingPrompt(f.Pending, time.Now())
	if response == nil {
		return nil, false
	}
	if validatePrompt(response) != nil {
		f.forget()
		return nil, false
	}
	return response, true
}

// save keeps response for a later process; a failure only costs that process the resume.
func (f DeviceFlow) save(response *oauth2.DeviceAuthResponse) {
	if f.Pending == "" {
		return
	}
	if err := authcache.SavePendingPrompt(f.Pending, response); err != nil && f.Logger != nil {
		f.Logger.Warn("sign-in prompt not saved for resuming", "diagnostic", authcache.FailureDiagnostic(err))
	}
}

// forget removes the saved prompt once its code is settled.
func (f DeviceFlow) forget() {
	if f.Pending == "" {
		return
	}
	if err := authcache.ForgetPendingPrompt(f.Pending); err != nil && f.Logger != nil {
		f.Logger.Warn("settled sign-in prompt not removed", "diagnostic", authcache.FailureDiagnostic(err))
	}
}

// deviceFailure is a failed device flow reduced to a credential-free reason for the log.
type deviceFailure struct {
	reason string
}

func (e *deviceFailure) Error() string { return errDeviceAuthorization.Error() + ": " + e.reason }

func (e *deviceFailure) Is(target error) bool { return target == errDeviceAuthorization }

// standardErrorCodes are the OAuth token errors of RFC 6749 section 5.2, RFC 8628 section 3.5,
// OpenID Connect and Microsoft's device-code flow that may be logged; any other
// provider-controlled value is reduced to a marker.
var standardErrorCodes = map[string]struct{}{
	"access_denied": {}, "account_selection_required": {}, "authorization_declined": {},
	"authorization_pending": {}, "bad_verification_code": {},
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

// Device-flow answers that tell oauth2 to keep polling, and to poll more slowly.
var (
	pendingPollResponse  = []byte(`{"error":"authorization_pending"}`)
	slowDownPollResponse = []byte(`{"error":"slow_down"}`)
)

// tolerantPollContext returns ctx with an OAuth HTTP client whose token polls survive every answer
// that leaves the device code unsettled. Mobile systems suspend the app while the player signs in
// through the browser, breaking or timing out in-flight polls, and networks answer with proxy and
// captive-portal pages. Reporting those as pending keeps oauth2's own polling loop, so slow_down
// backoff, rejection, cancellation and device-code expiry still end the flow. lostAnswer is set
// when a poll's answer never arrived, because Microsoft may have redeemed the code in that poll.
func tolerantPollContext(ctx context.Context, lostAnswer *atomic.Bool) context.Context {
	client, _ := ctx.Value(oauth2.HTTPClient).(*http.Client)
	if client == nil {
		client = http.DefaultClient
	}
	tolerant := *client
	tolerant.Transport = pollTransport{base: client.Transport, lostAnswer: lostAnswer}
	return context.WithValue(ctx, oauth2.HTTPClient, &tolerant)
}

// pollTransport passes on redirects and the answers that settle a device code: a token, or an
// OAuth error that is not temporary. A rate limit becomes slow_down and anything else pending.
type pollTransport struct {
	base       http.RoundTripper
	lostAnswer *atomic.Bool
}

func (t pollTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	base := t.base
	if base == nil {
		base = http.DefaultTransport
	}
	response, err := base.RoundTrip(request)
	if err != nil {
		t.lostAnswer.Store(true)
		return syntheticPoll(request, pendingPollResponse), nil
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, maxPollResponseBytes))
	_ = response.Body.Close()
	if err != nil {
		t.lostAnswer.Store(true)
		return syntheticPoll(request, pendingPollResponse), nil
	}
	if response.StatusCode == http.StatusTooManyRequests {
		return syntheticPoll(request, slowDownPollResponse), nil
	}
	if !redirects(response) && !settlesDeviceCode(response, body) {
		return syntheticPoll(request, pendingPollResponse), nil
	}
	response.Body = io.NopCloser(bytes.NewReader(body))
	response.ContentLength = int64(len(body))
	return response, nil
}

// redirects reports whether http.Client follows response to another request, which this transport
// then classifies in turn.
func redirects(response *http.Response) bool {
	switch response.StatusCode {
	case http.StatusMovedPermanently, http.StatusFound, http.StatusSeeOther,
		http.StatusTemporaryRedirect, http.StatusPermanentRedirect:
		return response.Header.Get("Location") != ""
	default:
		return false
	}
}

// settlesDeviceCode reports whether the provider itself answered the poll. Pages from proxies and
// captive portals carry neither a token nor an OAuth error, whatever their status.
func settlesDeviceCode(response *http.Response, body []byte) bool {
	if response.StatusCode >= http.StatusInternalServerError {
		return false
	}
	accessToken, code := tokenAnswer(response.Header.Get("Content-Type"), body)
	switch code {
	case "":
		return accessToken != "" && response.StatusCode >= http.StatusOK && response.StatusCode < http.StatusMultipleChoices
	case "server_error", "temporarily_unavailable":
		return false
	default:
		return true
	}
}

// tokenAnswer reads a token response's access token and OAuth error code from the same media
// types oauth2 reads; a body that does not decode has neither.
func tokenAnswer(contentType string, body []byte) (accessToken, code string) {
	media, _, _ := mime.ParseMediaType(contentType)
	if media == "application/x-www-form-urlencoded" || media == "text/plain" {
		values, err := url.ParseQuery(string(body))
		if err != nil {
			return "", ""
		}
		return values.Get("access_token"), values.Get("error")
	}
	var answer struct {
		AccessToken string `json:"access_token"`
		Error       string `json:"error"`
	}
	if json.Unmarshal(body, &answer) != nil {
		return "", ""
	}
	return answer.AccessToken, answer.Error
}

// syntheticPoll builds a device-flow error response for request with the given JSON body.
func syntheticPoll(request *http.Request, body []byte) *http.Response {
	return &http.Response{
		Status: "400 Bad Request", StatusCode: http.StatusBadRequest,
		Proto: "HTTP/1.1", ProtoMajor: 1, ProtoMinor: 1,
		Header:        http.Header{"Content-Type": {"application/json"}},
		Body:          io.NopCloser(bytes.NewReader(body)),
		ContentLength: int64(len(body)),
		Request:       request,
	}
}
