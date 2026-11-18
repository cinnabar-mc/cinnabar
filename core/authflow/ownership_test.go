package authflow

import (
	"bytes"
	"context"
	"errors"
	"io"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/df-mc/go-xsapi/v2/xal"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft/auth/mcservices"
	"golang.org/x/oauth2"
)

// fakeLicenses stands in for Microsoft sign-in and Minecraft services.
type fakeLicenses struct {
	t          *testing.T
	signIn     *oauth2.Token
	refreshErr error
	deviceErr  error  // refuses the device sign-in after showing its code
	onCheck    func() // runs as the entitlement check starts
	check      LicenseCheck
	checked    []string
}

func (f *fakeLicenses) DeviceAuth(context.Context) (*oauth2.DeviceAuthResponse, error) {
	if f.signIn == nil && f.deviceErr == nil {
		f.t.Fatal("ownership check started a sign-in")
	}
	return &oauth2.DeviceAuthResponse{VerificationURI: "https://login.example.test/device", UserCode: "ABCD-1234"}, nil
}

func (f *fakeLicenses) DeviceToken(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
	if f.deviceErr != nil {
		return nil, f.deviceErr
	}
	return f.signIn, nil
}

func (f *fakeLicenses) TokenSource(_ context.Context, token *oauth2.Token) oauth2.TokenSource {
	return tokenFunc(func() (*oauth2.Token, error) {
		if f.refreshErr != nil {
			return nil, f.refreshErr
		}
		return token, nil
	})
}

func (f *fakeLicenses) Check(_ context.Context, token *oauth2.Token) LicenseCheck {
	if f.onCheck != nil {
		f.onCheck()
	}
	f.checked = append(f.checked, token.RefreshToken)
	return f.check
}

type tokenFunc func() (*oauth2.Token, error)

func (f tokenFunc) Token() (*oauth2.Token, error) { return f() }

// seedLicense stores a license account whose access token has expired, so a check must refresh.
func seedLicense(t *testing.T, path, refresh string) {
	t.Helper()
	_, err := authcache.Source(context.Background(), authcache.Config{
		Path: path,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			return &oauth2.Token{AccessToken: "old", RefreshToken: refresh, Expiry: time.Now().Add(-time.Hour)}, nil
		},
		Refresh: staticRefresh,
	})
	if err != nil {
		t.Fatal(err)
	}
}

func runOwnership(t *testing.T, path string, signIn bool, licenses Licenses) []event {
	t.Helper()
	var output bytes.Buffer
	if err := Ownership(context.Background(), OwnershipConfig{Path: path, SignIn: signIn, Licenses: licenses, Writer: &output}); err != nil {
		t.Fatal(err)
	}
	for _, secret := range []string{"access-sentinel", "refresh-sentinel", "signed-sentinel"} {
		if strings.Contains(output.String(), secret) {
			t.Fatalf("ownership events leaked %q", secret)
		}
	}
	return decodeEvents(t, output.Bytes())
}

func last(events []event) event { return events[len(events)-1] }

func TestOwnershipWithoutALicenseAccountAsksForSignIn(t *testing.T) {
	path := filepath.Join(t.TempDir(), "license.json")
	licenses := &fakeLicenses{t: t}
	events := runOwnership(t, path, false, licenses)
	if got := last(events); got.Result != ResultSignInRequired || len(events) != 2 {
		t.Fatalf("events = %+v", events)
	}
	if len(licenses.checked) != 0 {
		t.Fatal("services were called without a license account")
	}
}

func TestOwnershipSignInPublishesTheCodeAndKeepsTheDocument(t *testing.T) {
	path := filepath.Join(t.TempDir(), "license.json")
	licenses := &fakeLicenses{
		t:      t,
		signIn: &oauth2.Token{AccessToken: "access-sentinel", RefreshToken: "refresh-sentinel", Expiry: time.Now().Add(time.Hour)},
		check:  LicenseCheck{Result: ResultVerified, Reason: "entitled", Signed: []byte("signed-sentinel")},
	}
	events := runOwnership(t, path, true, licenses)
	if events[1].Kind != "device_code" || last(events).Result != ResultVerified || last(events).Account == "" {
		t.Fatalf("events = %+v", events)
	}
	account := last(events).Account
	if saved, err := os.ReadFile(authcache.EntitlementsPath(path)); err != nil || string(saved) != "signed-sentinel" {
		t.Fatalf("entitlement document = %q, %v", saved, err)
	}
	// The stored account then checks without signing in again.
	licenses.signIn = nil
	licenses.check = LicenseCheck{Result: ResultNotOwned, Reason: "no_minecraft_entitlement"}
	if got := last(runOwnership(t, path, false, licenses)); got.Result != ResultNotOwned || got.Account != account {
		t.Fatalf("cached check = %+v, want account %q", got, account)
	}
}

func TestOwnershipSignInReplacesTheStoredAccount(t *testing.T) {
	path := filepath.Join(t.TempDir(), "license.json")
	seedLicense(t, path, "old-refresh")
	licenses := &fakeLicenses{
		t:      t,
		signIn: &oauth2.Token{AccessToken: "new", RefreshToken: "new-refresh", Expiry: time.Now().Add(time.Hour)},
		check:  LicenseCheck{Result: ResultVerified, Reason: "entitled"},
	}
	old := authcache.SignInGeneration(path)
	events := runOwnership(t, path, true, licenses)
	if events[1].Kind != "device_code" || len(licenses.checked) != 1 || licenses.checked[0] != "new-refresh" {
		t.Fatalf("events = %+v, checked = %v", events, licenses.checked)
	}
	if got := last(events).Account; got == "" || got == old {
		t.Fatalf("replacement reported account %q, old %q", got, old)
	}
}

// A result for an account replaced during its check names no account, so it certifies nobody.
func TestOwnershipDropsAResultWhoseAccountChanged(t *testing.T) {
	path := filepath.Join(t.TempDir(), "license.json")
	seedLicense(t, path, "refresh-sentinel")
	// Another account signs in while the entitlement check runs.
	licenses := &fakeLicenses{t: t, check: LicenseCheck{Result: ResultVerified, Reason: "entitled"}, onCheck: func() {
		if err := os.Remove(path); err != nil {
			t.Fatal(err)
		}
		seedLicense(t, path, "other-refresh")
	}}
	if got := last(runOwnership(t, path, false, licenses)); got.Result != ResultUnverifiable || got.Account != "" {
		t.Fatalf("result = %+v", got)
	}
}

// A device code alone replaces nothing: a sign-in that never completes keeps the stored account.
func TestOwnershipFailedSignInIsNotANewAccount(t *testing.T) {
	path := filepath.Join(t.TempDir(), "license.json")
	seedLicense(t, path, "old-refresh")
	// The player never approves the code.
	licenses := &fakeLicenses{t: t, deviceErr: &oauth2.RetrieveError{ErrorCode: "access_denied"}}
	old := authcache.SignInGeneration(path)
	events := runOwnership(t, path, true, licenses)
	if got := last(events); events[1].Kind != "device_code" || got.Result != ResultSignInRequired || got.Account != old {
		t.Fatalf("events = %+v, want account %q", events, old)
	}
	contents, err := os.ReadFile(path)
	if err != nil || !strings.Contains(string(contents), "old-refresh") {
		t.Fatalf("stored account changed: %v", err)
	}
}

// A refresh that never reached Microsoft says nothing about the account; a refused one needs sign-in.
func TestOwnershipRefreshFailuresSplitOfflineFromRejected(t *testing.T) {
	cases := []struct {
		err  error
		want string
	}{
		{&url.Error{Op: "Post", URL: "https://login.example.test", Err: errors.New("offline")}, ResultUnverifiable},
		{&oauth2.RetrieveError{ErrorCode: "invalid_grant"}, ResultSignInRequired},
		{&oauth2.RetrieveError{ErrorCode: "temporarily_unavailable"}, ResultUnverifiable},
	}
	for _, c := range cases {
		path := filepath.Join(t.TempDir(), "license.json")
		seedLicense(t, path, "refresh-sentinel")
		licenses := &fakeLicenses{t: t, refreshErr: c.err}
		if got := last(runOwnership(t, path, false, licenses)); got.Result != c.want {
			t.Fatalf("refresh error %v: result = %+v, want %s", c.err, got, c.want)
		}
		if len(licenses.checked) != 0 {
			t.Fatal("services were called without a token")
		}
	}
}

// Only an answer without a Minecraft item is not-owned; errors never are.
func TestEntitlementAnswersClassify(t *testing.T) {
	cases := []struct {
		entitlements *mcservices.Entitlements
		err          error
		want         string
	}{
		{&mcservices.Entitlements{Items: []mcservices.Item{{Name: "product_minecraft_bedrock"}}}, nil, ResultVerified},
		{&mcservices.Entitlements{Items: []mcservices.Item{{Name: "game_minecraft"}}}, nil, ResultVerified},
		{&mcservices.Entitlements{Items: []mcservices.Item{{Name: "product_dungeons"}}}, nil, ResultNotOwned},
		{&mcservices.Entitlements{}, nil, ResultNotOwned},
		{nil, mcservices.ErrUnauthorized, ResultSignInRequired},
		{nil, mcservices.ErrAppNotApproved, ResultUnverifiable},
		{nil, mcservices.ErrMalformedResponse, ResultUnverifiable},
		{nil, mcservices.ErrRequestIDMismatch, ResultUnverifiable},
		{nil, &mcservices.XboxError{XErr: mcservices.XErrChildAccount}, ResultUnverifiable},
		{nil, &xal.StatusError{StatusCode: 503}, ResultUnverifiable},
		{nil, context.DeadlineExceeded, ResultUnverifiable},
	}
	for _, c := range cases {
		if got := classifyEntitlements(c.entitlements, c.err); got.Result != c.want {
			t.Fatalf("classify(%v, %v) = %+v, want %s", c.entitlements, c.err, got, c.want)
		}
	}
	if got := classifyEntitlements(nil, &mcservices.XboxError{XErr: mcservices.XErrNoProfile}); got.Reason != "xerr_2148916233" {
		t.Fatalf("xbox reason = %q", got.Reason)
	}
}

// The launcher parses the same lines (crates/launcher-host ownership tests read this fixture).
func TestOwnershipEventsMatchTheSharedFixture(t *testing.T) {
	want, err := os.ReadFile(filepath.Join("testdata", "ownership_events.jsonl"))
	if err != nil {
		t.Fatal(err)
	}
	var output bytes.Buffer
	_ = emit(&output, event{Version: 1, Kind: "checking_cache"})
	_ = emit(&output, event{Version: 1, Kind: "device_code", VerificationURI: "https://login.example.test/device", UserCode: "ABCD-1234"})
	_ = finishOwnership(&output, nil, LicenseCheck{Result: ResultVerified, Reason: "entitled"}, "6f1c2a9e-0000-4000-8000-000000000001")
	for _, check := range []LicenseCheck{
		{Result: ResultNotOwned, Reason: "no_minecraft_entitlement"},
		{Result: ResultSignInRequired, Reason: "no_license_account"},
		{Result: ResultUnverifiable, Reason: "refresh:transport"},
	} {
		_ = finishOwnership(&output, nil, check, "")
	}
	// A Windows checkout may end the fixture's lines with CRLF.
	if output.String() != strings.ReplaceAll(string(want), "\r\n", "\n") {
		t.Fatalf("events =\n%s\nfixture =\n%s", output.String(), want)
	}
}
