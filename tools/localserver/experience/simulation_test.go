package experience

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// simulationFixture enables callbacks and removes the fixture's initial player observation.
func simulationFixture(t *testing.T) *runtimeFixture {
	t.Helper()
	f := newRuntimeFixture(t)
	f.d.sup.registered.Simulation = true
	<-f.d.events
	return f
}

// simulationEvent finds this instance's next event while retaining FIFO order within it. A tick it
// takes, or skips for another instance, takes its latest step and finishes as the worker's would.
func simulationEvent(t *testing.T, f *runtimeFixture, id string) event {
	t.Helper()
	for len(f.d.events) != 0 {
		ev := <-f.d.events
		if ev.call.Tick != nil {
			if !f.h.claimTick(f.d.id, &ev) {
				t.Fatal("a queued tick lost its opening")
			}
			f.h.finishTick(f.d.id, ev.instance.handle)
		}
		if string(ev.instance.handle.ID) == id {
			return ev
		}
	}
	t.Fatal("missing simulation event")
	return event{}
}

// applySimulation commits a result using the actual snapshot and validation path.
func applySimulation(t *testing.T, f *runtimeFixture, ev event, ops func(snapshot) []Op) snapshot {
	t.Helper()
	f.d.seq++
	snap, err := f.h.snapshot(t.Context(), f.d, &ev)
	if err != nil {
		t.Fatal(err)
	}
	f.do(ev.w, func(tx *world.Tx) {
		if _, err := f.h.apply(tx, f.d.id, ev, snap, ops(snap)); err != nil {
			t.Fatal(err)
		}
	})
	return snap
}

func TestSimulationCadenceAndTimerOrdering(t *testing.T) {
	f := simulationFixture(t)
	f.h.AdmitSimulationTick()
	ev := simulationEvent(t, f, "probe:one")
	if ev.call.Tick.Step.Tick != 1 || ev.call.Tick.Step.DurationNanos != uint64(tickDuration) {
		t.Fatal("wrong fixed step")
	}
	var handle ScheduleHandle
	applySimulation(t, f, ev, func(s snapshot) []Op {
		handle = ScheduleHandle{World: ev.instance.handle, Sequence: s.req.Info.Simulation.Sequence}
		return []Op{{Schedule: &ScheduleOp{handle, 2, 2}}}
	})
	f.h.AdmitSimulationTick()
	ev = simulationEvent(t, f, "probe:one")
	if ev.call.Timer != nil {
		t.Fatal("timer fired early")
	}
	f.h.AdmitSimulationTick()
	ev = simulationEvent(t, f, "probe:one")
	if ev.call.Tick == nil || ev.call.Tick.Step.Tick != 3 {
		t.Fatal("tick must precede timers")
	}
	ev = simulationEvent(t, f, "probe:one")
	if ev.call.Timer == nil || ev.call.Timer.Handle != handle || ev.call.Timer.Step.Tick != 3 {
		t.Fatal("missing due timer")
	}
	f.h.finishTimer(f.d.id, handle)
	f.h.AdmitSimulationTick()
	simulationEvent(t, f, "probe:one")
	f.h.AdmitSimulationTick()
	simulationEvent(t, f, "probe:one")
	ev = simulationEvent(t, f, "probe:one")
	if ev.call.Timer == nil || ev.call.Timer.Step.Tick != 5 {
		t.Fatal("repeat cadence changed")
	}
}

func TestSimulationCancelFencesQueuedTimer(t *testing.T) {
	f := simulationFixture(t)
	f.h.AdmitSimulationTick()
	ev := simulationEvent(t, f, "probe:one")
	var handle ScheduleHandle
	applySimulation(t, f, ev, func(s snapshot) []Op {
		handle = ScheduleHandle{World: ev.instance.handle, Sequence: s.req.Info.Simulation.Sequence}
		return []Op{{Schedule: &ScheduleOp{handle, 1, 0}}}
	})
	f.h.AdmitSimulationTick()
	ev = simulationEvent(t, f, "probe:one")
	applySimulation(t, f, ev, func(snapshot) []Op { return []Op{{CancelSchedule: &CancelScheduleOp{handle}}} })
	ev = simulationEvent(t, f, "probe:one")
	if ev.call.Timer == nil || f.h.timerCurrent(f.d.id, handle) {
		t.Fatal("cancelled queued timer retained authority")
	}
}

func TestSimulationPausesOnUnloadAndHostPause(t *testing.T) {
	f := simulationFixture(t)
	f.h.AdmitSimulationTick()
	ev := simulationEvent(t, f, "probe:one")
	old := ev.instance.handle
	if err := f.h.unloadWorld(t.Context(), f.d.id, old); err != nil {
		t.Fatal(err)
	}
	f.h.AdmitSimulationTick()
	f.h.AdmitSimulationTick()
	if f.h.simulation.ticks[old.ID] != 1 {
		t.Fatal("unloaded instance time advanced")
	}
	fresh, err := f.h.createWorld(t.Context(), f.d.id, WorldInstance{ID: old.ID, Definition: "probe:plain"})
	if err != nil {
		t.Fatal(err)
	}
	for len(f.d.events) > 0 {
		<-f.d.events
	}
	f.h.Pause(true)
	f.h.AdmitSimulationTick()
	if len(f.d.events) != 0 {
		t.Fatal("paused host admitted simulation")
	}
	f.h.Pause(false)
	f.h.AdmitSimulationTick()
	ev = simulationEvent(t, f, "probe:one")
	if ev.instance.handle != fresh || ev.call.Tick.Step.Tick != 2 {
		t.Fatal("reopened instance lost paused time or reused generation")
	}
	if _, err := f.h.snapshot(t.Context(), f.d, &event{w: ev.w, instance: worldInstance{handle: old}, call: ev.call}); !errors.Is(err, errStale) {
		t.Fatal("old opening retained callback authority")
	}
}

func TestSimulationOverflowIsNonblockingAndCounted(t *testing.T) {
	f := simulationFixture(t)
	for len(f.d.events) < cap(f.d.events) {
		f.d.events <- event{}
	}
	f.h.AdmitSimulationTick()
	if f.h.simulation.ticks[OverworldInstance] != 1 {
		t.Fatal("full queue stopped simulation")
	}
	if n := dropCountOf(&f.d.dropped); n == 0 {
		t.Fatal("overflow was not counted")
	}
}

// TestSimulationPendingTickTakesTheLatestStep keeps one tick per opening queued while it waits,
// counts the steps it skipped and delivers the newest one.
func TestSimulationPendingTickTakesTheLatestStep(t *testing.T) {
	f := simulationFixture(t)
	for range 3 {
		f.h.AdmitSimulationTick()
	}
	f.h.worlds.mu.RLock()
	openings := len(f.h.worlds.byWorld)
	f.h.worlds.mu.RUnlock()
	if n := len(f.d.events); n != openings {
		t.Fatalf("queued %d ticks for %d openings", n, openings)
	}
	if n := dropCountOf(&f.d.dropped); n != uint64(2*openings) {
		t.Fatalf("counted %d skipped ticks, want %d", n, 2*openings)
	}
	if ev := simulationEvent(t, f, "probe:one"); ev.call.Tick.Step.Tick != 3 {
		t.Fatalf("pending tick delivered step %d, want the latest", ev.call.Tick.Step.Tick)
	}
	f.h.AdmitSimulationTick()
	if ev := simulationEvent(t, f, "probe:one"); ev.call.Tick == nil || ev.call.Tick.Step.Tick != 4 {
		t.Fatal("a finished tick did not let the next step queue")
	}
}

// TestSimulationTicksCoalesceBehindASlowCallback floods ticks while a callback is slow: they must
// not fill the queue that the player's lifecycle events share, so the leave queues and is delivered.
func TestSimulationTicksCoalesceBehindASlowCallback(t *testing.T) {
	f := newRuntimeFixture(t)
	sup, _ := startFake(t, "leave_write", f.h.log, startOptions{})
	sup.registered.Worlds = f.d.sup.registered.Worlds
	sup.registered.Simulation = true
	f.d.sup = sup
	<-f.d.events
	entered, release, done := make(chan struct{}), make(chan struct{}), make(chan struct{})
	free := sync.OnceFunc(func() { close(release) })
	t.Cleanup(free)
	var slow sync.Once
	f.h.afterCall = func() { slow.Do(func() { close(entered); <-release }) }
	go func() { defer close(done); f.h.Run(context.Background()) }()
	f.h.AdmitSimulationTick()
	<-entered
	for range eventQueueCap {
		f.h.AdmitSimulationTick()
	}
	f.do(f.source, func(tx *world.Tx) {
		e, _ := f.actor.Entity(tx)
		f.h.PlayerDisconnected(e.(*player.Player))
	})
	if f.d.sup.Quarantined() {
		t.Fatal("ticks crowded out the player's leave and suspended the Experience")
	}
	if dropCountOf(&f.d.dropped) == 0 {
		t.Fatal("skipped ticks were not counted")
	}
	free()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := f.h.Drain(ctx); err != nil {
		t.Fatal(err)
	}
	<-done
	state := f.h.store.readStates("probe", []stateAddress{{Scope: StorePlayer, Owner: f.actor.UUID().String()}})[0]
	if state.Data == nil || *state.Data != "01" {
		t.Fatalf("the player's leave was not delivered: %+v", state)
	}
}

func TestSimulationTimerCommittedBeforeFirstTickSurvivesClockStart(t *testing.T) {
	f := simulationFixture(t)
	ev, snap := f.request()
	handle := ScheduleHandle{World: ev.instance.handle, Sequence: snap.req.Info.Simulation.Sequence}
	f.do(ev.w, func(tx *world.Tx) {
		if _, err := f.h.apply(tx, f.d.id, ev, snap, []Op{{Schedule: &ScheduleOp{handle, 1, 0}}}); err != nil {
			t.Fatal(err)
		}
	})
	f.h.AdmitSimulationTick()
	ev = simulationEvent(t, f, string(handle.World.ID))
	if ev.call.Tick == nil {
		t.Fatal("first tick missing")
	}
	ev = simulationEvent(t, f, string(handle.World.ID))
	if ev.call.Timer == nil || ev.call.Timer.Handle != handle {
		t.Fatal("clock initialization discarded committed timer")
	}
}

func TestSimulationUnloadStopsAdmissionBeforeRelease(t *testing.T) {
	f := simulationFixture(t)
	f.h.AdmitSimulationTick()
	ev := simulationEvent(t, f, "probe:one")
	handle := ScheduleHandle{World: ev.instance.handle, Sequence: 1}
	f.h.simulation.timers[simulationKey{f.d.id, handle.World}] = map[ScheduleHandle]simulationTimer{handle: {due: 2}}
	f.h.manager.afterUnbind = func() {
		f.h.AdmitSimulationTick()
		if f.h.simulation.ticks[handle.World.ID] != 1 || f.h.timerCurrent(f.d.id, handle) {
			t.Fatal("unloaded opening retained simulation authority before release")
		}
		if len(f.h.simulation.timers[simulationKey{f.d.id, handle.World}]) != 0 {
			t.Fatal("unload left timers alive until release")
		}
	}
	if err := f.h.unloadWorld(t.Context(), f.d.id, handle.World); err != nil {
		t.Fatal(err)
	}
}

func TestSimulationCancelledDispatchRetainsQueuedTimer(t *testing.T) {
	f := simulationFixture(t)
	f.h.AdmitSimulationTick()
	ev := simulationEvent(t, f, "probe:one")
	handle := ScheduleHandle{World: ev.instance.handle, Sequence: 1}
	f.h.simulation.timers[simulationKey{f.d.id, handle.World}] = map[ScheduleHandle]simulationTimer{handle: {pending: true}}
	ev.call = Call{Timer: &TimerCall{handle, ev.call.Tick.Step}}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if err := f.h.dispatch(ctx, f.d, ev); !errors.Is(err, context.Canceled) {
		t.Fatalf("dispatch = %v", err)
	}
	if !f.h.timerCurrent(f.d.id, handle) {
		t.Fatal("interrupted callback lost its retained timer")
	}
}

func TestSimulationTimerAcrossTransferKeepsInstanceAuthority(t *testing.T) {
	f := simulationFixture(t)
	if err := f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatal(err)
	}
	f.h.AdmitSimulationTick()
	ev := simulationEvent(t, f, "probe:one")
	var handle ScheduleHandle
	applySimulation(t, f, ev, func(s snapshot) []Op {
		handle = ScheduleHandle{World: ev.instance.handle, Sequence: s.req.Info.Simulation.Sequence}
		return []Op{{Schedule: &ScheduleOp{handle, 1, 0}}}
	})
	f.h.AdmitSimulationTick()
	simulationEvent(t, f, "probe:one")
	timer := simulationEvent(t, f, "probe:one")
	if err := f.travel(t.Context(), "probe:two"); err != nil {
		t.Fatal(err)
	}
	snap := applySimulation(t, f, timer, func(snapshot) []Op { return []Op{{CancelSchedule: &CancelScheduleOp{handle}}} })
	if snap.req.Actor != nil || snap.req.Info.Context.Player != nil || snap.req.Info.Context.Current != handle.World {
		t.Fatal("timer inherited the transferred player's destination authority")
	}
	if f.h.timerCurrent(f.d.id, handle) {
		t.Fatal("timer cancellation failed after transfer")
	}
}

func TestSimulationInterruptedTimerReplaysDuringDrain(t *testing.T) {
	m := newLifecycleMatrix(t)
	f := m.f
	for len(f.d.events) != 0 {
		<-f.d.events
	}
	f.h.AdmitSimulationTick()
	ev := simulationEvent(t, f, "probe:one")
	var handle ScheduleHandle
	applySimulation(t, f, ev, func(s snapshot) []Op {
		handle = ScheduleHandle{World: ev.instance.handle, Sequence: s.req.Info.Simulation.Sequence}
		return []Op{{Schedule: &ScheduleOp{handle, 1, 0}}}
	})
	for len(f.d.events) != 0 {
		<-f.d.events
	}
	f.h.AdmitSimulationTick()
	simulationEvent(t, f, "probe:one")
	timer := simulationEvent(t, f, "probe:one")
	for len(f.d.events) != 0 {
		<-f.d.events
	}
	f.d.events <- timer
	m.cancelCallback()
	if !f.h.timerCurrent(f.d.id, handle) {
		t.Fatal("interrupted helper consumed a timer before replay")
	}
	m.drain()
	if f.h.timerCurrent(f.d.id, handle) {
		t.Fatal("drain did not consume the retained timer")
	}
}

func TestSimulationFinalLeaveHasNoTimerAuthority(t *testing.T) {
	f := simulationFixture(t)
	if err := f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatal(err)
	}
	f.h.AdmitSimulationTick()
	ev := simulationEvent(t, f, "probe:one")
	applySimulation(t, f, ev, func(s snapshot) []Op {
		handle := ScheduleHandle{World: ev.instance.handle, Sequence: s.req.Info.Simulation.Sequence}
		return []Op{{Schedule: &ScheduleOp{handle, 1, 1}}}
	})
	f.do(ev.w, func(tx *world.Tx) {
		e, _ := f.actor.Entity(tx)
		f.h.PlayerDisconnected(e.(*player.Player))
	})
	var leave *event
	for len(f.d.events) != 0 {
		ev := <-f.d.events
		if ev.call.Player != nil && ev.call.Player.Event.Type == "leave" {
			leave = &ev
		}
	}
	if leave == nil {
		t.Fatal("missing final leave")
	}
	snap, err := f.h.snapshot(t.Context(), f.d, leave)
	if err != nil {
		t.Fatal(err)
	}
	if snap.req.Info.Simulation != nil {
		t.Fatal("leave granted timer staging that its final state commit cannot accept")
	}
	data := "01"
	if err := f.h.commitLeave(f.d.id, *leave, snap, []Op{{StoreWrite: &StoreWriteOp{Key: StoreKey{Scope: StorePlayer, Schema: 1}, Data: &data}}}); err != nil {
		t.Fatalf("final state commit: %v", err)
	}
}
