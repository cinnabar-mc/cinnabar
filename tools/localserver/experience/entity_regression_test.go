package experience

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"math"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/entity/networkoffset"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/chunk"
	"github.com/go-gl/mathgl/mgl64"
)

func TestEntityDespawnRetainsSurvivingSnapshots(t *testing.T) {
	f := newEntityFixture(t)
	removed, surviving := f.spawnEntity(), f.spawnEntity()
	f.do(f.source, func(tx *world.Tx) {
		e, ok := f.h.findEntity(tx, "probe", removed)
		if !ok {
			t.Fatal("entity missing before removal")
		}
		if err := e.Close(); err != nil {
			t.Fatal(err)
		}
	})
	var observed *event
	for len(f.d.events) > 0 {
		ev := <-f.d.events
		if ev.call.Entity != nil && ev.call.Entity.Event.Type == "despawned" {
			observed = &ev
		}
	}
	if observed == nil {
		t.Fatal("removal omitted its observation")
	}
	f.d.seq++
	snap, err := f.h.snapshot(t.Context(), f.d, observed)
	if err != nil {
		t.Fatal(err)
	}
	entities := snap.req.Info.Context.Entities
	if len(entities) != 1 || entities[0].State.Handle != surviving {
		t.Fatalf("live removal snapshot lost the surviving entity: %+v", entities)
	}
	state := entities[0].State
	state.Position.X++
	f.do(f.source, func(tx *world.Tx) {
		ops := []Op{{Entity: &EntityCommand{Action: EntityOp{Type: "update", State: &state}}}}
		if _, err := f.h.apply(tx, "probe", *observed, snap, ops); err != nil {
			t.Fatalf("removal callback cannot update a surviving entity: %v", err)
		}
	})
}

func TestHistoricalEntityRemovalFixtureRetainsStateOnlyAuthority(t *testing.T) {
	data, err := os.ReadFile("testdata/protocol/request_callback_entity_historical_removal.json")
	if err != nil {
		t.Fatal(err)
	}
	var request CallbackRequest
	if err := json.Unmarshal(data, &request); err != nil {
		t.Fatal(err)
	}
	context := request.Info.Context
	if context == nil || !context.OpeningClosed || len(context.Entities) != 0 || request.Actor != nil {
		t.Fatal("historical fixture retained live authority")
	}
}

func TestMovingEntitySurvivesChunkCleanupBeforeTick(t *testing.T) {
	f := newEntityFixture(t)
	synctest.Test(t, func(t *testing.T) {
		w := world.Config{ChunkUnloadInterval: time.Second}.New()
		w.SetPaused(true)
		defer w.Close()
		binding, err := f.h.BindWorld("probe:leases", w)
		if err != nil {
			t.Fatal(err)
		}
		defer func() {
			if err := f.h.UnbindWorld(t.Context(), binding); err != nil {
				t.Error(err)
			}
		}()
		instance, _ := f.h.worlds.of(w)
		state := EntityState{Handle: EntityHandle{1, instance.handle.Generation}, Kind: "probe:ship", Position: Position{Y: 70}, Scale: 1}
		handle := world.EntitySpawnOpts{Position: state.Position.vec()}.New(f.h.reg.entityTypes[state.Kind], entityConfig{state: state, instance: instance})
		f.do(w, func(tx *world.Tx) { tx.AddEntity(handle).(*modEntity).pin() })
		synctest.Wait()
		// Several guest moves may commit before Dragonfly migrates chunk membership.
		for _, x := range []float64{32, 64, 96} {
			state.Position.X = x
			f.do(w, func(tx *world.Tx) {
				e, ok := handle.Entity(tx)
				if !ok {
					t.Error("actor disappeared before moving")
					return
				}
				e.(*modEntity).update(state, true)
			})
			synctest.Wait()
		}
		<-time.After(2 * time.Second)
		synctest.Wait()
		f.do(w, func(tx *world.Tx) {
			e, ok := handle.Entity(tx)
			if !ok || e.Position() != state.Position.vec() {
				t.Error("cleanup closed the moving actor before chunk migration")
				return
			}
			if _, loaded := tx.BlockLoaded(cube.Pos{32, 70, 0}); loaded {
				t.Error("intermediate destination retained an unnecessary lease")
			}
		})
		if t.Failed() {
			return
		}
		w.AdvanceTick()
		<-time.After(2 * time.Second)
		synctest.Wait()
		f.do(w, func(tx *world.Tx) {
			if _, ok := handle.Entity(tx); !ok {
				t.Error("cleanup closed the actor after chunk migration")
				return
			}
			if _, loaded := tx.BlockLoaded(cube.Pos{0, 70, 0}); loaded {
				t.Error("old chunk lease survived completed migration")
			}
		})
		f.do(w, func(tx *world.Tx) {
			e, ok := handle.Entity(tx)
			if !ok {
				t.Error("actor missing before removal")
				return
			}
			state.Position.X = 128
			e.(*modEntity).update(state, false)
			_ = e.Close()
		})
		<-time.After(2 * time.Second)
		synctest.Wait()
		f.do(w, func(tx *world.Tx) {
			for _, x := range []int{96, 128} {
				if _, loaded := tx.BlockLoaded(cube.Pos{x, 70, 0}); loaded {
					t.Errorf("removed actor retained chunk lease at x=%d", x)
				}
			}
		})
	})
}

// TestEntitySpawnOrdinalsSurviveOtherOperations validates the guest's coalesced operation stream.
func TestEntitySpawnOrdinalsSurviveOtherOperations(t *testing.T) {
	for _, sequence := range []uint64{1, math.MaxUint64 / maxStagedOps} {
		t.Run(fmt.Sprint(sequence), func(t *testing.T) {
			f := newEntityFixture(t)
			ev, snap := f.request()
			snap.req.Info.EventSequence = sequence
			first := EntityState{Handle: EntityHandle{sequence*maxStagedOps + 1, ev.instance.handle.Generation}, Kind: "probe:ship", Position: Position{Y: 70}, Scale: 1, Properties: []EntityProperty{{ID: "probe:throttle"}}}
			second := first
			second.Handle.ID++
			ops := []Op{
				{Entity: &EntityCommand{Action: EntityOp{Type: "spawn", State: &first}}},
				{SetBlock: &SetBlockOp{Pos: BlockPos{Y: 70}, ID: airID}},
				{Entity: &EntityCommand{Action: EntityOp{Type: "despawn", Handle: first.Handle}}},
				{Entity: &EntityCommand{Action: EntityOp{Type: "spawn", State: &second}}},
			}
			f.do(ev.w, func(tx *world.Tx) {
				if err := f.h.validateEntities(tx, "probe", ev, snap, nil, ops); err != nil {
					t.Fatalf("valid coalesced entity batch rejected: %v", err)
				}
				bad := second
				bad.Handle.ID++
				ops[3].Entity.Action.State = &bad
				if err := f.h.validateEntities(tx, "probe", ev, snap, nil, ops); !errors.Is(err, errInvalid) {
					t.Fatalf("unreserved identity accepted: %v", err)
				}
				snap.req.Info.EventSequence = math.MaxUint64/maxStagedOps + 1
				if err := f.h.validateEntities(tx, "probe", ev, snap, nil, ops); !errors.Is(err, errInvalid) {
					t.Fatalf("overflowed reservation accepted: %v", err)
				}
			})
		})
	}
}

// motionViewer models the session's reset of actor motion during a teleport.
type motionViewer struct {
	world.NopViewer
	velocity mgl64.Vec3
	position mgl64.Vec3
}

// ViewEntityVelocity retains the latest motion received by a viewer.
func (v *motionViewer) ViewEntityVelocity(_ world.Entity, velocity mgl64.Vec3) { v.velocity = velocity }

// ViewEntityTeleport models the motion reset sent by a session for a teleport.
func (v *motionViewer) ViewEntityTeleport(_ world.Entity, position mgl64.Vec3) {
	v.position = position
	v.velocity = mgl64.Vec3{}
}

// ViewEntityMovement retains ordinary movement without resetting motion.
func (v *motionViewer) ViewEntityMovement(_ world.Entity, position mgl64.Vec3, _ cube.Rotation, _ bool) {
	v.position = position
}

func TestEntityTeleportPreservesRequestedVelocity(t *testing.T) {
	for _, teleport := range []bool{false, true} {
		t.Run(fmt.Sprint(teleport), func(t *testing.T) {
			f := newEntityFixture(t)
			v := &motionViewer{}
			f.do(f.source, func(tx *world.Tx) {
				loader := world.NewLoader(0, f.source, v)
				loader.Move(tx, mgl64.Vec3{0, 70, 0})
				loader.Load(tx, 1)
				t.Cleanup(func() {
					if err := f.source.Do(func(tx *world.Tx) { loader.Close(tx) }).Wait(context.Background()); err != nil {
						t.Error(err)
					}
				})
			})
			handle := f.spawnEntity()
			f.d.seq++
			ev, snap := f.request()
			state := snap.req.Info.Context.Entities[0].State
			state.Position.X = 2
			state.Velocity = Position{X: 0.25, Y: 0.5, Z: -0.125}
			f.do(ev.w, func(tx *world.Tx) {
				ops := []Op{{Entity: &EntityCommand{Action: EntityOp{Type: "update", State: &state, Teleport: teleport}}}}
				if _, err := f.h.apply(tx, "probe", ev, snap, ops); err != nil {
					t.Fatal(err)
				}
				actor, ok := f.h.findEntity(tx, "probe", handle)
				if !ok || actor.Velocity() != state.Velocity.vec() {
					t.Fatal("server lost requested velocity")
				}
			})
			if v.position != state.Position.vec() || v.velocity != state.Velocity.vec() {
				t.Fatalf("viewer position/motion = %v/%v, want %v/%v", v.position, v.velocity, state.Position.vec(), state.Velocity.vec())
			}
		})
	}
}

// failedEntityDestination rejects the first asynchronous read of the teleport destination.
type failedEntityDestination struct {
	world.NopProvider
	attempts atomic.Int32
}

// LoadColumn lets origin reads generate normally and makes one destination read fail.
func (p *failedEntityDestination) LoadColumn(pos world.ChunkPos, dim world.Dimension) (*chunk.Column, error) {
	if pos == (world.ChunkPos{2, 0}) && p.attempts.Add(1) == 1 {
		return nil, errors.New("destination temporarily unavailable")
	}
	return p.NopProvider.LoadColumn(pos, dim)
}

func TestEntityTeleportRetriesWithoutDestinationTicks(t *testing.T) {
	f := newEntityFixture(t)
	synctest.Test(t, func(t *testing.T) {
		provider := &failedEntityDestination{}
		w := world.Config{Provider: provider, ChunkLoadWorkers: 1, Log: slog.New(slog.DiscardHandler)}.New()
		w.SetPaused(true)
		defer w.Close()
		binding, err := f.h.BindWorld("probe:retry", w)
		if err != nil {
			t.Fatal(err)
		}
		defer func() {
			if err := f.h.UnbindWorld(t.Context(), binding); err != nil {
				t.Error(err)
			}
		}()
		instance, _ := f.h.worlds.of(w)
		state := EntityState{Handle: EntityHandle{1, binding.Generation}, Kind: "probe:ship", Position: Position{Y: 70}, Scale: 1}
		handle := world.EntitySpawnOpts{Position: state.Position.vec()}.New(f.h.reg.entityTypes[state.Kind], entityConfig{state: state, instance: instance})
		f.do(w, func(tx *world.Tx) { tx.AddEntity(handle).(*modEntity).pin() })
		state.Position.X = 32
		f.do(w, func(tx *world.Tx) { e, _ := handle.Entity(tx); e.(*modEntity).update(state, true) })
		synctest.Wait()
		w.AdvanceTick()
		<-time.After(2 * tickDuration)
		synctest.Wait()
		f.do(w, func(tx *world.Tx) {
			if _, loaded := tx.BlockLoaded(cube.Pos{32, 70, 0}); !loaded {
				t.Error("one-shot teleport did not retry the missing destination")
			}
			if _, loaded := tx.BlockLoaded(cube.Pos{0, 70, 0}); !loaded {
				t.Error("retry released the owning chunk before membership migrated")
			}
		})
		w.AdvanceTick()
		f.do(w, func(tx *world.Tx) {
			e, ok := handle.Entity(tx)
			if !ok || e.Position() != state.Position.vec() {
				t.Error("retried actor lost its authoritative position")
			}
		})
	})
}

func TestEntityAppearancePreservesNetworkCoordinates(t *testing.T) {
	def := entityDefinition()
	typ, err := newModEntityType("probe", def, newAssetCache())
	if err != nil {
		t.Fatal(err)
	}
	offset, ok := any(typ).(session.OffsetEntity)
	if !ok {
		t.Fatal("entity type does not provide its selected base's packet offset")
	}
	position := mgl64.Vec3{3, 70, 4}
	encoded := position.Add(mgl64.Vec3{0, offset.NetworkOffset(), 0})
	decoded := encoded.Sub(mgl64.Vec3{0, networkoffset.Entity(def.Appearance.ID), 0})
	if decoded != position {
		t.Fatalf("base-actor coordinate round trip: %v, want %v", decoded, position)
	}
}

func TestEntitySnapshotRetainsForeignVehicleOccupancy(t *testing.T) {
	f := newEntityFixture(t)
	f.spawnEntity()
	def := renamed(Loaded{ID: "probe", Entities: []EntityDefinition{entityDefinition()}}, "other").Entities[0]
	typ, err := newModEntityType("other", def, newAssetCache())
	if err != nil {
		t.Fatal(err)
	}
	typ.host = f.h
	instance, _ := f.h.worlds.of(f.source)
	state := EntityState{Handle: EntityHandle{1, instance.handle.Generation}, Kind: def.ID, Position: Position{Y: 70}, Scale: 1}
	vehicle := world.EntitySpawnOpts{Position: state.Position.vec()}.New(typ, entityConfig{state: state, instance: instance})
	f.do(f.source, func(tx *world.Tx) {
		p, _ := f.actor.Entity(tx)
		e := tx.AddEntity(vehicle).(*modEntity)
		e.pin()
		if !tx.Mount(p, e, 0) {
			t.Fatal("foreign vehicle mount failed")
		}
		f.h.reconcilePlayer(p.(*player.Player), true)
	})
	var observed *event
	for len(f.d.events) != 0 {
		ev := <-f.d.events
		if ev.call.Player != nil {
			observed = &ev
		}
	}
	if observed == nil {
		t.Fatal("foreign rider movement was not observed")
	}
	historical, err := f.h.snapshot(t.Context(), f.d, observed)
	if err != nil || !historical.req.Info.Context.ActorRidingElsewhere {
		t.Fatalf("player observation lost foreign occupancy: %v", err)
	}
	_, snap := f.request()
	if !snap.req.Info.Context.ActorRidingElsewhere || len(snap.req.Info.Context.Entities) != 1 {
		t.Fatalf("snapshot lost global occupancy or exposed a foreign vehicle: %+v", snap.req.Info.Context)
	}
}

func TestForeignVehicleFixtureRetainsAdmissionStatus(t *testing.T) {
	body, err := os.ReadFile(filepath.Join("testdata", "protocol", "request_callback_entity_foreign_vehicle.json"))
	if err != nil {
		t.Fatal(err)
	}
	var request Request
	if err := json.Unmarshal(body, &request); err != nil {
		t.Fatal(err)
	}
	ctx := request.Callback.Info.Context
	if ctx == nil || !ctx.ActorRidingElsewhere {
		t.Fatal("cross-language fixture lost foreign riding status")
	}
	for _, e := range ctx.Entities {
		for _, rider := range e.Riders {
			if request.Callback.Actor != nil && rider.Player == *request.Callback.Actor {
				t.Fatal("foreign vehicle leaked into the mod's entity snapshot")
			}
		}
	}
}

// TestEntityConsecutiveChunkUpdatesReachPublishedViewers keeps clients current before migration.
func TestEntityConsecutiveChunkUpdatesReachPublishedViewers(t *testing.T) {
	for _, teleport := range []bool{false, true} {
		t.Run(fmt.Sprint(teleport), func(t *testing.T) {
			f := newEntityFixture(t)
			viewer := &motionViewer{}
			f.do(f.source, func(tx *world.Tx) {
				loader := world.NewLoader(0, f.source, viewer)
				loader.Move(tx, mgl64.Vec3{0, 70, 0})
				loader.Load(tx, 1)
				t.Cleanup(func() {
					if err := f.source.Do(func(tx *world.Tx) { loader.Close(tx) }).Wait(context.Background()); err != nil {
						t.Error(err)
					}
				})
			})
			handle := f.spawnEntity()
			f.do(f.source, func(tx *world.Tx) {
				actor, ok := f.h.findEntity(tx, "probe", handle)
				if !ok {
					t.Fatal("actor missing before movement")
				}
				state := actor.snapshot().State
				for _, x := range []float64{32, 0} {
					state.Position.X = x
					actor.update(state, teleport)
					if viewer.position != state.Position.vec() {
						t.Fatalf("viewer position = %v after movement to %v", viewer.position, state.Position.vec())
					}
				}
			})
		})
	}
}
