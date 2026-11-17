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

var (
	errNoLicense       = errors.New("no license account")
	errLicenseRejected = errors.New("license account sign-in rejected")
	errLicenseOffline  = errors.New("license account could not refresh")
)

// Ownership checks whether the license account owns Minecraft and emits one terminal
// ownership event, marked signed_in once a new account's credentials are saved; tokens and
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
		return finishOwnership(writer, config.Logger, LicenseCheck{Result: ResultUnverifiable, Reason: "configuration"}, false)
	}
	licenses := config.Licenses
	skipCached := config.SignIn
	signedIn := false
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
			switch {
			case refreshErr == nil:
				return nil, errNoLicense
			case credentialsRejected(refreshErr):
				return nil, errLicenseRejected
			default:
				return nil, errLicenseOffline
			}
		}
		flow := DeviceFlow{Authorize: licenses.DeviceAuth, Token: licenses.DeviceToken, Pending: config.Path, Logger: config.Logger}
		token, err := flow.Request(ctx, func(response *oauth2.DeviceAuthResponse) error {
			return emit(writer, event{Version: 1, Kind: "device_code", VerificationURI: response.VerificationURI, UserCode: response.UserCode})
		})
		signedIn = err == nil
		return token, err
	}
	source, err := authcache.Source(ctx, authcache.Config{Path: config.Path, Writer: io.Discard, Request: request, Refresh: refresh})
	if err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return finishOwnership(writer, config.Logger, signInFailure(err, refreshErr), false)
	}
	// Source saved any new account's credentials before returning.
	token, err := source.Token()
	if err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return finishOwnership(writer, config.Logger, LicenseCheck{Result: ResultUnverifiable, Reason: "token:" + failureReason(err)}, signedIn)
	}
	checkCtx, cancelCheck := context.WithTimeout(ctx, checkTimeout)
	check := licenses.Check(checkCtx, token)
	cancelCheck()
	if ctx.Err() != nil {
		return ctx.Err()
	}
	if len(check.Signed) != 0 {
		if err := authcache.SaveEntitlements(config.Path, check.Signed); err != nil && config.Logger != nil {
			config.Logger.Warn("entitlement document not saved", "diagnostic", authcache.FailureDiagnostic(err))
		}
	}
	return finishOwnership(writer, config.Logger, check, signedIn)
}

// signInFailure classifies why no license token was available.
func signInFailure(err, refreshErr error) LicenseCheck {
	var device *deviceFailure
	switch {
	case errors.Is(err, errNoLicense):
		return LicenseCheck{Result: ResultSignInRequired, Reason: "no_license_account"}
	case errors.Is(err, errLicenseRejected):
		return LicenseCheck{Result: ResultSignInRequired, Reason: "refresh:" + failureReason(refreshErr)}
	case errors.Is(err, errLicenseOffline):
		return LicenseCheck{Result: ResultUnverifiable, Reason: "refresh:" + failureReason(refreshErr)}
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
func finishOwnership(writer io.Writer, logger *slog.Logger, check LicenseCheck, signedIn bool) error {
	if logger != nil {
		logger.Info("ownership checked", "result", check.Result, "reason", check.Reason, "signed_in", signedIn)
	}
	return emit(writer, event{Version: 1, Kind: "ownership", Result: check.Result, Reason: check.Reason, SignedIn: signedIn})
}
