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
	for _, library := range []struct {
		module string
		// required forks must be replaced in core; others may resolve upstream everywhere.
		required bool
	}{
		{module: "github.com/sandertv/gophertunnel", required: true},
		{module: "github.com/sandertv/go-raknet", required: true},
		{module: "github.com/df-mc/go-nethernet"},
	} {
		t.Run(path.Base(library.module), func(t *testing.T) {
			core := forkPin(t, "go.mod", library.module)
			if core == "" && library.required {
				t.Fatalf("core/go.mod does not replace %s with the fork", library.module)
			}
			for _, module := range modules {
				if pin := forkPin(t, module, library.module); pin != "" && pin != core {
					t.Errorf("%s replaces %s with %q, core with %q", module, library.module, pin, core)
				}
			}
		})
	}
}

// forkPin reads the replacement selected for one transport module, or "" when it is not replaced.
func forkPin(t *testing.T, manifest, module string) string {
	t.Helper()
	data, err := os.ReadFile(manifest)
	if err != nil {
		t.Fatal(err)
	}
	pattern := regexp.MustCompile(fmt.Sprintf(`(?m)^\s*(?:replace\s+)?%s\s*=>\s*(\S+\s+\S+)`, regexp.QuoteMeta(module)))
	match := pattern.FindSubmatch(data)
	if match == nil {
		return ""
	}
	return string(match[1])
}
