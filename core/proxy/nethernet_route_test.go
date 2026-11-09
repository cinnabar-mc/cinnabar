package proxy

import (
	"bytes"
	"log/slog"
	"strings"
	"testing"
	"time"

	"github.com/pion/ice/v4"
	"github.com/pion/logging"
)

// testRoutePair returns a pair from this device's reflexive address to a server host candidate.
func testRoutePair(t *testing.T) *ice.CandidatePair {
	t.Helper()
	local, err := ice.NewCandidateServerReflexive(&ice.CandidateServerReflexiveConfig{
		Network: "udp", Address: "203.0.113.7", Port: 50000, Component: 1, RelAddr: "192.168.1.20", RelPort: 50000,
	})
	if err != nil {
		t.Fatal(err)
	}
	remote, err := ice.NewCandidateHost(&ice.CandidateHostConfig{Network: "udp", Address: "198.51.100.4", Port: 19132, Component: 1})
	if err != nil {
		t.Fatal(err)
	}
	pair := &ice.CandidatePair{Local: local, Remote: remote}
	pair.UpdateRoundTripTime(35 * time.Millisecond)
	return pair
}

func TestRouteLoggerReportsSelectedPair(t *testing.T) {
	var out bytes.Buffer
	factory := routeLoggerFactory{log: slog.New(slog.NewTextHandler(&out, nil)), LoggerFactory: logging.NewDefaultLoggerFactory()}
	factory.NewLogger("ice").Tracef(selectedPairFormat, testRoutePair(t))

	line := out.String()
	for _, want := range []string{"NetherNet route selected", "local=srflx", "remote=host", "network=udp4", "rtt_ms=35"} {
		if !strings.Contains(line, want) {
			t.Fatalf("route log %q is missing %q", line, want)
		}
	}
	if strings.Contains(line, "198.51.100.4") {
		t.Fatalf("route log %q exposes the server address", line)
	}
}

func TestRouteLoggerIgnoresOtherMessagesAndScopes(t *testing.T) {
	var out bytes.Buffer
	factory := routeLoggerFactory{log: slog.New(slog.NewTextHandler(&out, nil)), LoggerFactory: logging.NewDefaultLoggerFactory()}
	pair := testRoutePair(t)
	factory.NewLogger("ice").Tracef("Ping STUN from %s to %s", pair.Local, pair.Remote)
	factory.NewLogger("sctp").Tracef(selectedPairFormat, pair)
	if out.Len() != 0 {
		t.Fatalf("unexpected route log %q", out.String())
	}
}
