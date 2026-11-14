package core_test

import (
	"fmt"
	"os"
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
	examples, err := filepath.Glob("../examples/*/go.mod")
	if err != nil {
		t.Fatal(err)
	}
	modules = append(modules, examples...)
	for _, fork := range []struct {
		upstream string
		library  string
	}{
		{upstream: "sandertv/gophertunnel", library: "gophertunnel"},
		{upstream: "sandertv/go-raknet", library: "go-raknet"},
		{upstream: "df-mc/go-nethernet", library: "go-nethernet"},
		{upstream: "pion/webrtc/v4", library: "webrtc/v4"},
	} {
		t.Run(fork.upstream, func(t *testing.T) {
			core := forkPin(t, "go.mod", fork.upstream, fork.library)
			if core == "" {
				t.Fatalf("core/go.mod does not replace %s with the fork", fork.upstream)
			}
			for _, module := range modules {
				if pin := forkPin(t, module, fork.upstream, fork.library); pin != "" && pin != core {
					t.Errorf("%s pins %q, core pins %q", module, pin, core)
				}
			}
		})
	}
}

// forkPin reads the hashimthearab fork that a module manifest replaces upstream (owner/name) with.
func forkPin(t *testing.T, manifest, upstream, library string) string {
	t.Helper()
	data, err := os.ReadFile(manifest)
	if err != nil {
		t.Fatal(err)
	}
	pattern := regexp.MustCompile(fmt.Sprintf(`github\.com/%s\s*=>\s*(github\.com/hashimthearab/%s\s+\S+)`, regexp.QuoteMeta(upstream), regexp.QuoteMeta(library)))
	match := pattern.FindSubmatch(data)
	if match == nil {
		return ""
	}
	return string(match[1])
}
