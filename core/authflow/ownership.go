package authflow

import (
	"context"
	"errors"
	"io"
	"log/slog"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"golang.org/x/oauth2"
)

// Ownership results, matching the launcher's ownership check.
const (
	ResultVerified       = "verified"
	ResultNotOwned       = "not_owned"
	ResultSignInRequired = "sign_in_required"
	ResultUnverifiable   = "unverifiable"
)

// Licenses is the license account's Microsoft app and the Minecraft services check behind it.
type Licenses interface {
	DeviceAuth(ctx context.Context) (*oauth2.DeviceAuthResponse, error)
	DeviceToken(ctx context.Context, response *oauth2.DeviceAuthResponse) (*oauth2.Token, error)
	TokenSource(ctx context.Context, token *oauth2.Token) oauth2.TokenSource
	Check(ctx context.Context, token *oauth2.Token) LicenseCheck
}

// LicenseCheck is one entitlement answer: a result, a credential-free reason, and the signed
// document to keep, if any.
type LicenseCheck struct {
	Result, Reason string
	Signed         []byte
}

// OwnershipConfig configures one ownership check of the license account cached at Path.
type OwnershipConfig struct {
	Path string
	// SignIn replaces any stored license account with a new device sign-in.
	SignIn   bool
	Licenses Licenses
	Writer   io.Writer
	Logger   *slog.Logger
}

// checkTimeout bounds each non-interactive exchange: the token refresh and the entitlement check.
const checkTimeout = 60 * time.Second

// licenseUnavailable is why a check without a new sign-in got no license token.
type licenseUnavailable struct{ check LicenseCheck }

func (e *licenseUnavailable) Error() string { return "license account unavailable: " + e.check.Reason }

// Ownership checks whether the license account owns Minecraft and emits one terminal
// ownership event naming the checked sign-in generation, or none when it is unknown; tokens and
// entitlement payloads never reach the stream or the log.
func Ownership(ctx context.Context, config OwnershipConfig) error {
	writer := config.Writer
	if writer == nil {
		writer = io.Discard
	}
	if err := emit(writer, event{Version: 1, Kind: "checking_cache"}); err != nil {
		return err
	}
	if config.Path == "" || config.Licenses == nil {
		return finishOwnership(writer, config.Logger, LicenseCheck{Result: ResultUnverifiable, Reason: "configuration"}, "")
	}
	finish := func(check LicenseCheck) error {
		return finishOwnership(writer, config.Logger, check, authcache.SignInGeneration(config.Path))
	}
	licenses := config.Licenses
	skipCached := config.SignIn
	var refreshErr error
	refreshCtx, cancelRefresh := context.WithTimeout(ctx, checkTimeout)
	defer cancelRefresh()
	refresh := func(token *oauth2.Token, _ io.Writer) oauth2.TokenSource {
		if skipCached {
			skipCached = false
			return nil
		}
		return recordingSource{source: licenses.TokenSource(refreshCtx, token), failure: &refreshErr}
	}
	request := func(ctx context.Context, _ io.Writer) (*oauth2.Token, error) {
		skipCached = false
		if !config.SignIn {
			return nil, &licenseUnavailable{check: refreshFailure(refreshErr)}
		}
		flow := DeviceFlow{Authorize: licenses.DeviceAuth, Token: licenses.DeviceToken, Pending: config.Path, Logger: config.Logger}
		return flow.Request(ctx, func(response *oauth2.DeviceAuthResponse) error {
			return emit(writer, event{Version: 1, Kind: "device_code", VerificationURI: response.VerificationURI, UserCode: response.UserCode})
		})
	}
	source, err := authcache.Source(ctx, authcache.Config{Path: config.Path, Writer: io.Discard, Request: request, Refresh: refresh})
	if err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return finish(signInFailure(err))
	}
	token, err := source.Token()
	if err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return finish(LicenseCheck{Result: ResultUnverifiable, Reason: "token:" + failureReason(err)})
	}
	// The source's own sign-in, which its token always belongs to; rereading the cache could name a replacement.
	account := authcache.Generation(source)
	checkCtx, cancelCheck := context.WithTimeout(ctx, checkTimeout)
	check := licenses.Check(checkCtx, token)
	cancelCheck()
	if ctx.Err() != nil {
		return ctx.Err()
	}
	if authcache.SignInGeneration(config.Path) != account {
		return finishOwnership(writer, config.Logger, LicenseCheck{Result: ResultUnverifiable, Reason: "account_changed"}, "")
	}
	if len(check.Signed) != 0 {
		if err := authcache.SaveEntitlements(config.Path, check.Signed); err != nil && config.Logger != nil {
			config.Logger.Warn("entitlement document not saved", "diagnostic", authcache.FailureDiagnostic(err))
		}
	}
	return finishOwnership(writer, config.Logger, check, account)
}

// refreshFailure classifies a stored license account that gave no token after refreshErr, which
// is nil when no account is stored.
func refreshFailure(refreshErr error) LicenseCheck {
	switch {
	case refreshErr == nil:
		return LicenseCheck{Result: ResultSignInRequired, Reason: "no_license_account"}
	case credentialsRejected(refreshErr):
		return LicenseCheck{Result: ResultSignInRequired, Reason: "refresh:" + failureReason(refreshErr)}
	default:
		return LicenseCheck{Result: ResultUnverifiable, Reason: "refresh:" + failureReason(refreshErr)}
	}
}

// signInFailure classifies why no license token was available.
func signInFailure(err error) LicenseCheck {
	var unavailable *licenseUnavailable
	var device *deviceFailure
	switch {
	case errors.As(err, &unavailable):
		return unavailable.check
	case errors.As(err, &device):
		return LicenseCheck{Result: ResultSignInRequired, Reason: device.reason}
	default:
		return LicenseCheck{Result: ResultUnverifiable, Reason: authcache.FailureDiagnostic(err)}
	}
}

// credentialsRejected reports whether Microsoft refused the stored refresh token, rather than
// being unreachable.
func credentialsRejected(err error) bool {
	var provider *oauth2.RetrieveError
	if !errors.As(err, &provider) {
		return false
	}
	switch provider.ErrorCode {
	case "invalid_grant", "interaction_required", "consent_required", "login_required":
		return true
	}
	return false
}

// recordingSource keeps the last refresh failure so a missing token can be told apart from an
// unreachable service.
type recordingSource struct {
	source  oauth2.TokenSource
	failure *error
}

func (s recordingSource) Token() (*oauth2.Token, error) {
	token, err := s.source.Token()
	if err != nil {
		*s.failure = err
	}
	return token, err
}

// finishOwnership logs and emits the terminal ownership event.
func finishOwnership(writer io.Writer, logger *slog.Logger, check LicenseCheck, account string) error {
	if logger != nil {
		logger.Info("ownership checked", "result", check.Result, "reason", check.Reason)
	}
	return emit(writer, event{Version: 1, Kind: "ownership", Result: check.Result, Reason: check.Reason, Account: account})
}
