package proxy

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net"
	"os"
	"path/filepath"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

func TestSessionPrepareKeepsSavedPacksOutOfHandoff(t *testing.T) {
	// Exercise the executable path even when an implementation suppresses it in test binaries.
	executable := os.Args[0]
	os.Args[0] = "cinnabar-core"
	t.Cleanup(func() { os.Args[0] = executable })
	for _, test := range []struct {
		name          string
		revisions     int
		customProfile bool
	}{
		{name: "active_globals", revisions: 1},
		{name: "retained_revision", revisions: 2},
		{name: "custom_profile", revisions: 1, customProfile: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			appData := t.TempDir()
			t.Setenv("LOCALAPPDATA", appData)
			t.Setenv("CINNABAR_USER_ROOT", "")
			if test.customProfile {
				// The selected profile has no globals; the default profile has an unrelated selection.
				t.Setenv("CINNABAR_USER_ROOT", t.TempDir())
			}
			pack := testAdmissionPack(t)
			writeSavedPackTestCatalog(t, filepath.Join(appData, "Cinnabar", "resource-packs", "global"), pack.UUID().String(), test.revisions)
			negotiated := negotiatedOfferUpstream(t, []*resource.Pack{pack}, false)
			stack, err := captureSelectedResourcePackStack(negotiated, nil)
			if err != nil {
				t.Fatal(err)
			}
			upstream := newFakeUpstream(nil)
			upstream.useBatchReads = true
			upstream.batchReads <- batchResult{packets: []packet.Packet{&packet.StartGame{EntityRuntimeID: 9}}}
			logger := slog.New(slog.NewTextHandler(io.Discard, nil))
			connections := newPreparedConnections("", nil, logger)
			connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
				return &preparedConnection{upstream: upstream, packStack: stack}, nil
			}
			server := &sessionServer{prepared: connections, transfers: new(TransferState), logger: logger}
			local, peer := net.Pipe()
			defer peer.Close()
			session := newSessionConn(streamnet.NewFramedConn(local))
			defer session.Close()
			ctx, cancel := context.WithCancelCause(context.Background())
			defer cancel(nil)
			downstream, err := newSessionDownstream(testSessionConnect(t), nil)
			if err != nil {
				t.Fatal(err)
			}
			plan, prepared, err := server.prepare(ctx, cancel, session, downstream, nil)
			if err != nil {
				t.Fatal(err)
			}
			defer prepared.close()
			if len(plan.handoff.Packs) != 1 || len(plan.packs) != 1 {
				t.Fatalf("handoff has %d entries and %d archives, want only the server pack", len(plan.handoff.Packs), len(plan.packs))
			}
			selected := plan.handoff.Packs[0]
			if selected.UUID != pack.UUID().String() || selected.Version != pack.Version() || selected.SubPack != "" {
				t.Fatalf("saved selection replaced the server entry: %+v", selected)
			}
			archive, err := io.ReadAll(io.NewSectionReader(plan.packs[0], 0, int64(plan.packs[0].Size())))
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.Equal(archive, testAdmissionPackArchive(t)) {
				t.Fatal("saved selection replaced the server archive")
			}
		})
	}
}

// writeSavedPackTestCatalog creates an active update and any older revision retained for restore.
func writeSavedPackTestCatalog(t *testing.T, root, id string, revisions int) {
	t.Helper()
	if err := os.MkdirAll(root, 0o700); err != nil {
		t.Fatal(err)
	}
	var available, retained []map[string]any
	for revision := 1; revision <= revisions; revision++ {
		version := revision + 1
		archive := admissionPackArchiveWithVersion(t, id, fmt.Sprintf("%d, 0, 0", version))
		name := fmt.Sprintf("%s-%d.0.0-%d.mcpack", id, version, revision)
		if err := os.WriteFile(filepath.Join(root, name), archive, 0o600); err != nil {
			t.Fatal(err)
		}
		metadata := map[string]any{
			"id":          id,
			"version":     []int{version, 0, 0},
			"revision":    revision,
			"name":        "saved",
			"description": "saved",
			"subpacks":    []any{},
		}
		if revision == revisions {
			available = append(available, metadata)
		} else {
			retained = append(retained, metadata)
		}
	}
	if retained == nil {
		retained = []map[string]any{}
	}
	restore := []map[string]any{}
	if revisions > 1 {
		restore = append(restore, map[string]any{"id": id, "revision": 1})
	}
	catalog, err := json.Marshal(map[string]any{
		"available":     available,
		"retained":      retained,
		"next_revision": revisions,
		"active":        []map[string]any{{"id": id, "revision": revisions}},
		"restore":       restore,
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "global_packs.json"), catalog, 0o600); err != nil {
		t.Fatal(err)
	}
}
