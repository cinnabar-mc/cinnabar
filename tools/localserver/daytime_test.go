package main

import (
	"io"
	"testing"

	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

func TestParseTimeOfDay(t *testing.T) {
	for in, want := range map[string]int{"noon": 6000, "midnight": 18000, "0": 0, "30000": 30000} {
		if got, err := parseTimeOfDay(in); err != nil || got != want {
			t.Fatalf("parseTimeOfDay(%q) = %d, %v; want %d", in, got, err, want)
		}
	}
	for _, in := range []string{"", "-1", "dusk", "1.5"} {
		if _, err := parseTimeOfDay(in); err == nil {
			t.Fatalf("parseTimeOfDay(%q) accepted", in)
		}
	}
}

func TestTimeFlagFreezesEveryOpenedWorld(t *testing.T) {
	base := []string{"-dir", "d", "-addr", "127.0.0.1:1"}
	if _, err := parseSettings(append(base, "-time", "dusk"), io.Discard); err == nil {
		t.Fatal("-time dusk accepted")
	}
	s, err := parseSettings(append(base, "-time", "noon"), io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	w := world.Config{Synchronous: true}.New()
	t.Cleanup(func() { _ = w.Close() })
	s.applyTo(w)
	if w.TimeCycle() || w.Time() != namedTimes["noon"] {
		t.Fatalf("time %d, cycle %v; want %d frozen", w.Time(), w.TimeCycle(), namedTimes["noon"])
	}

	untouched := world.Config{Synchronous: true}.New()
	t.Cleanup(func() { _ = untouched.Close() })
	s.timeOfDay = ""
	s.applyTo(untouched)
	if !untouched.TimeCycle() {
		t.Fatal("no -time stopped the daylight cycle")
	}
}

func TestTimeCommandSetsFreezesAndResumes(t *testing.T) {
	withPlayer(t, world.GameModeCreative, func(tx *world.Tx, p *player.Player) {
		w := tx.World()
		check := func(args string, ticks int, cycle bool) {
			t.Helper()
			runCommand(t, tx, p, "time", args)
			if w.Time() != ticks || w.TimeCycle() != cycle {
				t.Fatalf("/time %s: time %d, cycle %v; want %d, %v", args, w.Time(), w.TimeCycle(), ticks, cycle)
			}
		}
		check("set midnight", namedTimes["midnight"], false)
		check("set 1234", 1234, false)
		check("set -5", 1234, false) // a refused time leaves the world alone
		check("cycle", 1234, true)
	})
}
