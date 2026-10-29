package control

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

// stubLAN lists fixed LAN worlds beside the stub catalog.
type stubLAN struct {
	stubServices
	worlds []bridgecontract.LANWorld
}

func (s *stubLAN) LANWorlds(context.Context) ([]bridgecontract.LANWorld, error) {
	return s.worlds, s.err
}

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

// A listing too large for one frame arrives over several pages of one snapshot, losing no world,
// and a page of a replaced snapshot is refused.
func TestLANWorldsPagesALargeListing(t *testing.T) {
	worlds := make([]bridgecontract.LANWorld, 40)
	for i := range worlds {
		worlds[i] = bridgecontract.LANWorld{Address: fmt.Sprintf("lan/%d", i+1), LevelName: strings.Repeat("w", 4000)}
	}
	dir := startServices(t, NewStore(), &stubLAN{worlds: worlds})
	var got []bridgecontract.LANWorld
	var first lanWorldsResultV1
	if reply := rpc(t, dir, methodLANWorlds, ""); reply.Error != nil || json.Unmarshal(reply.Result, &first) != nil || first.Next == 0 {
		t.Fatalf("first page = %d worlds, next %d / %+v", len(first.Worlds), first.Next, reply.Error)
	}
	got = append(got, first.Worlds...)
	for page := first; page.Next != 0; {
		reply := rpc(t, dir, methodLANWorlds, fmt.Sprintf(`{"snapshot":%d,"offset":%d}`, page.Snapshot, page.Next))
		page = lanWorldsResultV1{}
		if reply.Error != nil || json.Unmarshal(reply.Result, &page) != nil || page.Snapshot != first.Snapshot {
			t.Fatalf("later page = %+v / %+v", page, reply.Error)
		}
		got = append(got, page.Worlds...)
	}
	if len(got) != len(worlds) || got[len(got)-1].Address != "lan/40" {
		t.Fatalf("paged %d worlds, want %d in order", len(got), len(worlds))
	}
	_ = rpc(t, dir, methodLANWorlds, "")
	if reply := rpc(t, dir, methodLANWorlds, fmt.Sprintf(`{"snapshot":%d,"offset":%d}`, first.Snapshot, first.Next)); reply.Error == nil {
		t.Fatal("served a page of a replaced listing")
	}
}

// A world too large for a frame by itself is still listed, with its text shortened and its address kept.
func TestLANWorldsShortensAnOversizedWorld(t *testing.T) {
	world := bridgecontract.LANWorld{Address: "lan/7", LevelName: strings.Repeat("\u0001", frameBudget), ServerName: "Host"}
	page := lanWorldsPage(1, []bridgecontract.LANWorld{world}, 0)
	if len(page.Worlds) != 1 || page.Next != 0 || page.Worlds[0].Address != "lan/7" || page.Worlds[0].ServerName != "Host" {
		t.Fatalf("page = %d worlds, next %d", len(page.Worlds), page.Next)
	}
	if len(page.Worlds[0].LevelName) > maxLANTextBytes || encodedLen(page) > frameBudget {
		t.Fatalf("level name of %d bytes, page of %d", len(page.Worlds[0].LevelName), encodedLen(page))
	}
}
