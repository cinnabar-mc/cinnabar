package authcache

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"testing"
	"time"

	"golang.org/x/oauth2"
)

// TestPendingPromptMatchesTheClientFixture keeps the saved prompt readable by the Rust client,
// whose launcher::pending_sign_in tests read the same fixture.
func TestPendingPromptMatchesTheClientFixture(t *testing.T) {
	path := filepath.Join(t.TempDir(), "token.json")
	if err := SavePendingPrompt(path, &oauth2.DeviceAuthResponse{
		DeviceCode: "device-ABCD-1234", UserCode: "ABCD-1234", VerificationURI: "https://login.example.test/device",
		Expiry: time.Unix(1893456000, 0), Interval: 5,
	}); err != nil {
		t.Fatal(err)
	}
	decode := func(file string) map[string]any {
		contents, err := os.ReadFile(file)
		if err != nil {
			t.Fatal(err)
		}
		var decoded map[string]any
		if err := json.Unmarshal(contents, &decoded); err != nil {
			t.Fatal(err)
		}
		return decoded
	}
	saved, fixture := decode(PendingPromptPath(path)), decode(filepath.Join("testdata", "pending_prompt.json"))
	if !reflect.DeepEqual(saved, fixture) {
		t.Fatalf("saved prompt = %v, want the client fixture %v", saved, fixture)
	}
	if info, err := os.Stat(PendingPromptPath(path)); err != nil || checkCachePermissions(info) != nil {
		t.Fatalf("saved prompt is not private: %v", err)
	}
}
