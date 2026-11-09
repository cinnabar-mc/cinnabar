package authcache

import (
	_ "embed"
	"encoding/json"
	"errors"
	"io/fs"
	"os"
	"time"

	"golang.org/x/oauth2"
)

//go:embed pending_prompt_suffix.txt
var pendingPromptSuffix string

// maxPendingPromptSize bounds a saved device prompt, which holds only short provider strings.
const maxPendingPromptSize = 8 * 1024

// pendingPrompt is a device prompt saved while the player approves it in the browser. Expiry is
// absolute, unlike the provider's expires_in, so a later process can tell whether it still holds.
type pendingPrompt struct {
	DeviceCode              string `json:"device_code"`
	UserCode                string `json:"user_code"`
	VerificationURI         string `json:"verification_uri"`
	VerificationURIComplete string `json:"verification_uri_complete,omitempty"`
	ExpiresAt               int64  `json:"expires_at"`
	Interval                int64  `json:"interval,omitempty"`
}

// PendingPromptPath returns where the device prompt awaiting approval for the token cache at
// oauthPath is saved.
func PendingPromptPath(oauthPath string) string {
	return oauthPath + pendingPromptSuffix
}

// SavePendingPrompt privately saves response for oauthPath, so a sign-in whose process the system
// ended, as phones do to apps left for the browser, resumes polling the same code the next time
// the player signs in. A prompt without an expiry is not saved, since a later process could not
// tell it had lapsed.
func SavePendingPrompt(oauthPath string, response *oauth2.DeviceAuthResponse) error {
	if response == nil || response.Expiry.IsZero() {
		return nil
	}
	serialized, err := json.Marshal(pendingPrompt{
		DeviceCode: response.DeviceCode, UserCode: response.UserCode,
		VerificationURI: response.VerificationURI, VerificationURIComplete: response.VerificationURIComplete,
		ExpiresAt: response.Expiry.Unix(), Interval: response.Interval,
	})
	if err != nil {
		return err
	}
	if err := savePrivate(PendingPromptPath(oauthPath), serialized); err != nil {
		return &cacheFailure{operation: "save_pending_prompt", err: err}
	}
	return nil
}

// LoadPendingPrompt returns the saved prompt for oauthPath while it is still valid at now, or
// nil. An expired or unreadable prompt is removed.
func LoadPendingPrompt(oauthPath string, now time.Time) *oauth2.DeviceAuthResponse {
	path := PendingPromptPath(oauthPath)
	contents, err := loadPrivate(path, maxPendingPromptSize)
	if errors.Is(err, fs.ErrNotExist) {
		return nil
	}
	var saved pendingPrompt
	if err != nil || json.Unmarshal(contents, &saved) != nil || saved.DeviceCode == "" ||
		!now.Before(time.Unix(saved.ExpiresAt, 0)) {
		_ = ForgetPendingPrompt(oauthPath)
		return nil
	}
	return &oauth2.DeviceAuthResponse{
		DeviceCode: saved.DeviceCode, UserCode: saved.UserCode,
		VerificationURI: saved.VerificationURI, VerificationURIComplete: saved.VerificationURIComplete,
		Expiry: time.Unix(saved.ExpiresAt, 0), Interval: saved.Interval,
	}
}

// ForgetPendingPrompt removes the saved prompt for oauthPath, if any.
func ForgetPendingPrompt(oauthPath string) error {
	if err := os.Remove(PendingPromptPath(oauthPath)); err != nil && !errors.Is(err, fs.ErrNotExist) {
		return &cacheFailure{operation: "forget_pending_prompt", err: err}
	}
	return nil
}
