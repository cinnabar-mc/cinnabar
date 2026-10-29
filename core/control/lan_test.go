package control

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

// stubLAN lists fixed LAN worlds beside the stub catalog.
type stubLAN struct {
	stubServices
	worlds []bridgecontract.LANWorld
}

func (s *stubLAN) LANWorlds(context.Context) ([]bridgecontract.LANWorld, error) { return s.worlds, s.err }

// lan_worlds.v1 encodes the rows exactly as the shared fixture the Rust bridge decodes.
func TestLANWorldsMatchesTheSharedFixture(t *testing.T) {
	fixture, err := os.ReadFile("../../crates/bridge/data/lan-worlds.json")
	if err != nil {
		t.Fatal(err)
	}
	var want lanWorldsResultV1
	if err := json.Unmarshal(fixture, &want); err != nil {
		t.Fatal(err)
	}
	dir := startServices(t, NewStore(), &stubLAN{worlds: want.Worlds})
	reply := rpc(t, dir, methodLANWorlds, "")
	if reply.Error != nil {
		t.Fatalf("lan_worlds.v1 error = %+v", reply.Error)
	}
	var compactWant, compactGot bytes.Buffer
	if err := json.Compact(&compactWant, fixture); err != nil {
		t.Fatal(err)
	}
	if err := json.Compact(&compactGot, reply.Result); err != nil {
		t.Fatal(err)
	}
	if compactGot.String() != compactWant.String() {
		t.Fatalf("lan_worlds.v1 = %s\nwant %s", compactGot.String(), compactWant.String())
	}
}

// An empty list is an empty array, parameters are refused, and a core without LAN reports it.
func TestLANWorldsEdges(t *testing.T) {
	dir := startServices(t, NewStore(), &stubLAN{})
	var result lanWorldsResultV1
	if reply := rpc(t, dir, methodLANWorlds, ""); reply.Error != nil || json.Unmarshal(reply.Result, &result) != nil ||
		result.Worlds == nil || len(result.Worlds) != 0 {
		t.Fatalf("empty LAN list = %+v / %+v", result, reply.Error)
	}
	if reply := rpc(t, dir, methodLANWorlds, `{"x":1}`); reply.Error == nil || reply.Error.Code != bridgecontract.RPCInvalidParams {
		t.Fatalf("params must be rejected: %+v", reply.Error)
	}
	plain := startServices(t, NewStore(), &stubServices{})
	if reply := rpc(t, plain, methodLANWorlds, ""); reply.Error == nil || reply.Error.Code != codeServicesDisabled {
		t.Fatalf("no LAN backend = %+v", reply.Error)
	}
}

// connect.v1 accepts a LAN world target.
func TestConnectAcceptsLANTarget(t *testing.T) {
	stub := &stubLAN{}
	dir := startServices(t, NewStore(), stub)
	if reply := rpc(t, dir, methodConnect, `{"kind":"lan","value":"12345"}`); reply.Error != nil || stub.kind != TargetLAN || stub.value != "12345" {
		t.Fatalf("connect lan = %+v, kind %q value %q", reply.Error, stub.kind, stub.value)
	}
}
