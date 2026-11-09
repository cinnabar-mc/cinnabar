package experience

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/world"
)

// probeContainer moves one item between the first two slots of the container above it.
const probeContainer = 37

// A guest reads the chest above its block and moves one stone between its first two slots; the
// commit writes the chest. A chest changed after the snapshot leaves the result stale.
func TestGuestMovesItemsInAChest(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	at := probePos(probeContainer)
	above := BlockPos{X: at.X, Y: at.Y + 1, Z: at.Z}
	f.place(probeCounter, at, nil)
	stone, _ := world.ItemByName("minecraft:stone", 0)
	f.do(func(tx *world.Tx) {
		chest := block.NewChest()
		tx.SetBlock(above.cube(), chest, nil)
		_ = chest.Inventory(tx, above.cube()).SetItem(0, item.NewStack(stone, 3))
	})
	f.moveTo(beside(at))
	f.activate(at)
	f.waitTells(5*time.Second, "container 27 moved minecraft:stone 2 1")
	f.do(func(tx *world.Tx) {
		inv := tx.Block(above.cube()).(block.Chest).Inventory(tx, above.cube())
		first, _ := inv.Item(0)
		second, _ := inv.Item(1)
		if first.Count() != 2 || second.Count() != 1 {
			t.Fatalf("the chest holds %v and %v, want 2 and 1 stone", first, second)
		}
	})
}

// A snapshot lists a double chest once, at the half it reaches first with the other as its pair,
// an unloaded position as unloaded, and no block that holds no container.
func TestContainerSnapshotListsADoubleChestOnce(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	at := probePos(probeContainer)
	f.place(probeCounter, at, nil)
	east := BlockPos{X: at.X + 1, Y: at.Y, Z: at.Z}
	south := BlockPos{X: east.X, Y: east.Y, Z: east.Z + 1}
	f.do(func(tx *world.Tx) {
		// Each half saved as paired with the other, as a placed double chest saves them.
		half := func(pair BlockPos) block.Chest {
			return block.Chest{}.DecodeNBT(map[string]any{"pairx": pair.X, "pairz": pair.Z}).(block.Chest)
		}
		tx.SetBlock(east.cube(), half(south), nil)
		tx.SetBlock(south.cube(), half(east), nil)
	})
	snap := f.snapshotOf(Call{Neighbor: &NeighborCall{Pos: at, Neighbor: at}}, at)
	got := snap.req.Containers
	// The block north of the anchor is in a chunk the fixture never loads.
	north := BlockPos{X: at.X, Y: at.Y, Z: at.Z - 1}
	if got == nil || got.Truncated || len(got.Blocks) != 2 {
		t.Fatalf("containers = %+v, want the double chest and the unloaded %v", got, north)
	}
	var chest, unloaded *Container
	for i := range got.Blocks {
		switch got.Blocks[i].Pos {
		case east:
			chest = &got.Blocks[i]
		case north:
			unloaded = &got.Blocks[i]
		}
	}
	if unloaded == nil || unloaded.Loaded || unloaded.Pair != nil || len(unloaded.Slots) != 0 {
		t.Fatalf("containers = %+v, want %v listed as unloaded", got, north)
	}
	if chest == nil || !chest.Loaded || chest.Pair == nil || *chest.Pair != south || len(chest.Slots) != 54 {
		t.Fatalf("containers = %+v, want the double chest at %v paired with %v", got, east, south)
	}
	if _, ok := snap.containers[east.cube()]; !ok {
		t.Fatal("the snapshot keeps no state for the listed chest")
	}
}

// At the bounds, maxContainers double chests whose stacks carry maxContainerDataBytes of data
// under the longest item ids, the containers fit the frame's reserve for them.
func TestContainersAtTheirBoundsFitTheirReserve(t *testing.T) {
	id := strings.Repeat("e", 32) + ":" + strings.Repeat("i", 32)
	stacks := maxContainers * maxContainerSlots
	data := strings.Repeat("ab", maxContainerDataBytes/stacks+1)
	out := Containers{Blocks: make([]Container, maxContainers), Truncated: true}
	for i := range out.Blocks {
		pair := BlockPos{X: -30_000_000, Y: -64, Z: -30_000_000}
		c := Container{Pos: BlockPos{X: -30_000_000, Y: -64, Z: int32(i)}, Loaded: true, Pair: &pair}
		for range maxContainerSlots {
			c.Slots = append(c.Slots, &ItemStack{ID: id, Metadata: 32767, Count: 64, MaxCount: 64, Data: &data})
		}
		out.Blocks[i] = c
	}
	encoded, err := json.Marshal(out)
	if err != nil {
		t.Fatal(err)
	}
	if len(encoded) > containerFrameReserve {
		t.Fatalf("containers at the bounds take %d bytes; the reserve is %d", len(encoded), containerFrameReserve)
	}
	t.Logf("containers at the bounds: %d of %d bytes", len(encoded), containerFrameReserve)
}
