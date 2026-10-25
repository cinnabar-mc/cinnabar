package main

import (
	"io"
	"slices"
	"sync"
	"testing"
	"testing/synctest"
	"time"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/block/model"
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
)

// While the client part is missing, every failed send (commands, bind pulses, radar updates)
// tells the player once; a send that goes through again re-arms the warning.
func TestMaterialFxWarnsOnceWhileTheClientPartIsMissing(t *testing.T) {
	running := false
	told := 0
	c := &materialFxConn{
		ext: &experienceSender{send: func(uuid.UUID, string, string, uint16, []cinnabarext.Scalar) bool { return running }},
		// Only the warning reaches the player here, so each lookup is one message.
		players: func(uuid.UUID) (*world.EntityHandle, bool) { told++; return nil, false },
	}
	for range 2 * materialFxBindPulses {
		c.send(materialFxRecord(materialFxBind), materialFxRecord(materialFxRadar, 0, 0, 0, 1))
	}
	if told != 1 {
		t.Fatalf("told the player %d times while the client part was missing, want once", told)
	}
	running = true
	c.send(materialFxRecord(materialFxBind))
	running = false
	c.send(materialFxRecord(materialFxBind))
	c.send(materialFxRecord(materialFxBind))
	if told != 2 {
		t.Fatalf("told the player %d times after the client part went away again, want twice", told)
	}
}

// materialFxTestBlock stands in for one of the Experience's gallery blocks.
type materialFxTestBlock struct {
	name string
	hash uint64
}

func (b materialFxTestBlock) EncodeBlock() (string, map[string]any) { return b.name, nil }
func (b materialFxTestBlock) Hash() (uint64, uint64)                { return b.hash, 0 }
func (materialFxTestBlock) Model() world.BlockModel                 { return model.Solid{} }

// Respawning the gallery elsewhere clears the previous gallery's blocks, keeps those the new
// gallery reuses, and leaves alone any block a player has since put in an old gallery spot.
func TestMaterialFxRespawnClearsThePreviousGallery(t *testing.T) {
	registry := world.NewBlockRegistry()
	registered := map[string]bool{}
	for _, name := range materialFxBlocks(showcaseViewpoint{}) {
		if !registered[name] {
			registered[name] = true
			registry.RegisterBlockState(world.BlockState{Name: name})
			registry.RegisterBlock(materialFxTestBlock{name: name, hash: block.NextHash()})
		}
	}
	registry.Finalize()
	w := world.Config{Synchronous: true, Entities: entity.DefaultRegistry, Blocks: registry}.New()
	t.Cleanup(func() { _ = w.Close() })
	handle := world.EntitySpawnOpts{Position: mgl64.Vec3{0, 100, 0}}.New(player.Type, player.Config{Name: "tester"})
	if err := w.Do(func(tx *world.Tx) { tx.AddEntity(handle) }).Err(); err != nil {
		t.Fatal(err)
	}
	c := &materialFxConn{id: handle.UUID(), players: func(uuid.UUID) (*world.EntityHandle, bool) { return handle, true }}
	viewAt := func(x float32, yaw float32) showcaseViewpoint {
		return viewpointOf(&packet.PlayerAuthInput{Position: mgl32.Vec3{x, 64 + actorBurstEyeHeight, 0}, Yaw: yaw})
	}
	first, second := viewAt(0, 0), viewAt(3, 90)
	c.placeBlocks(nil, first)
	var built cube.Pos
	for pos := range materialFxBlocks(first) {
		if _, reused := materialFxBlocks(second)[pos]; !reused {
			built = pos
			break
		}
	}
	if err := w.Do(func(tx *world.Tx) { tx.SetBlock(built, block.Stone{}, nil) }).Err(); err != nil {
		t.Fatal(err)
	}
	c.placeBlocks(&first, second)
	if err := w.Do(func(tx *world.Tx) {
		blocks := materialFxBlocks(second)
		for pos, name := range materialFxBlocks(first) {
			got, _ := tx.Block(pos).EncodeBlock()
			want := "minecraft:air"
			if pos == built {
				want = "minecraft:stone"
			} else if replaced, ok := blocks[pos]; ok {
				want = replaced
			}
			if got != want {
				t.Fatalf("old %s at %v is %s, want %s", name, pos, got, want)
			}
		}
		for pos, name := range blocks {
			if got, _ := tx.Block(pos).EncodeBlock(); got != name {
				t.Fatalf("new %s at %v is %s", name, pos, got)
			}
		}
	}).Err(); err != nil {
		t.Fatal(err)
	}
}

// materialFxTestConn replays `packets` and accepts every write.
type materialFxTestConn struct {
	session.Conn
	packets []packet.Packet
}

// ReadPacket returns the next queued packet, then io.EOF.
func (c *materialFxTestConn) ReadPacket() (packet.Packet, error) {
	if len(c.packets) == 0 {
		return nil, io.EOF
	}
	pk := c.packets[0]
	c.packets = c.packets[1:]
	return pk, nil
}

// WritePacket drops `pk`.
func (c *materialFxTestConn) WritePacket(packet.Packet) error { return nil }

// Portal distances reach the client part in the order the player moved, even when sending an
// earlier one is held up, so the last distance delivered is the player's current one.
func TestMaterialFxPortalDistancesKeepTheirOrder(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		gallery := showcaseViewpoint{}
		portal := materialFxPieces(gallery)[materialFxPortalPiece]
		var inputs []packet.Packet
		var want []int64
		for _, x := range []float32{0, 20, 40} {
			eye := mgl32.Vec3{x, 64 + actorBurstEyeHeight, 0}
			inputs = append(inputs, &packet.PlayerAuthInput{Position: eye})
			want = append(want, milli(portal.Sub(eye).Len()))
		}
		var mu sync.Mutex
		var sent []int64
		ext := &experienceSender{send: func(_ uuid.UUID, _, _ string, _ uint16, record []cinnabarext.Scalar) bool {
			distance := *record[1].Integer
			if distance == want[0] {
				// Hold the first update as a contended client part connection would.
				held := make(chan struct{})
				time.AfterFunc(time.Second, func() { close(held) })
				<-held
			}
			mu.Lock()
			sent = append(sent, distance)
			mu.Unlock()
			return true
		}}
		c := &materialFxConn{
			Conn:     &materialFxTestConn{packets: inputs},
			ext:      ext,
			everyone: &materialFxPlayers{positions: map[uuid.UUID]mgl32.Vec3{}},
			done:     make(chan struct{}),
			gallery:  &gallery,
		}
		for range inputs {
			if _, err := c.ReadPacket(); err != nil {
				t.Fatal(err)
			}
		}
		settled := make(chan struct{})
		time.AfterFunc(2*time.Second, func() { close(settled) })
		<-settled
		synctest.Wait()
		mu.Lock()
		defer mu.Unlock()
		if !slices.Equal(sent, want) {
			t.Fatalf("sent distances %v, want %v in order", sent, want)
		}
	})
}
