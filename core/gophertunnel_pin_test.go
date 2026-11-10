package core_test

import (
	"fmt"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"testing"
)

// Each transport consumer must use the same fork commits as the client core.
func TestModulesPinTheCoreTransportForks(t *testing.T) {
	modules, err := filepath.Glob("../tools/*/go.mod")
	if err != nil {
		t.Fatal(err)
	}
	for _, upstream := range []string{"sandertv/gophertunnel", "sandertv/go-raknet", "df-mc/go-nethernet"} {
		t.Run(upstream, func(t *testing.T) {
			core := forkPin(t, "go.mod", upstream)
			if core == "" {
				t.Fatalf("core/go.mod does not replace %s with the fork", upstream)
			}
			for _, module := range modules {
				if pin := forkPin(t, module, upstream); pin != "" && pin != core {
					t.Errorf("%s pins %q, core pins %q", module, pin, core)
				}
			}
		})
	}
}

// forkPin reads the hashimthearab fork that a module manifest replaces upstream (owner/name) with.
func forkPin(t *testing.T, manifest, upstream string) string {
	t.Helper()
	data, err := os.ReadFile(manifest)
	if err != nil {
		t.Fatal(err)
	}
	library := path.Base(upstream)
	pattern := regexp.MustCompile(fmt.Sprintf(`github\.com/%s\s*=>\s*(github\.com/hashimthearab/%s\s+\S+)`, regexp.QuoteMeta(upstream), regexp.QuoteMeta(library)))
	match := pattern.FindSubmatch(data)
	if match == nil {
		return ""
	}
	return string(match[1])
}
