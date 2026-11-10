package main

import (
	"fmt"
	"maps"
	"slices"
	"strconv"

	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/world"
)

// namedTimes are the times of day Bedrock's /time set accepts by name, in ticks.
var namedTimes = map[string]int{
	"sunrise":  23000,
	"day":      1000,
	"noon":     6000,
	"sunset":   12000,
	"night":    13000,
	"midnight": 18000,
}

// parseTimeOfDay reads a time of day: one of namedTimes or a tick count of 0 or more.
func parseTimeOfDay(s string) (int, error) {
	if ticks, ok := namedTimes[s]; ok {
		return ticks, nil
	}
	ticks, err := strconv.Atoi(s)
	if err != nil || ticks < 0 {
		return 0, fmt.Errorf("time %q is neither a tick count of 0 or more nor one of %v", s, timeNames())
	}
	return ticks, nil
}

// timeNames lists namedTimes in a stable order.
func timeNames() []string {
	return slices.Sorted(maps.Keys(namedTimes))
}

// freezeTime sets w's time and stops its daylight cycle, so captures see the same sky.
func freezeTime(w *world.World, ticks int) {
	w.StopTime()
	w.SetTime(ticks)
}

// timeName is the /time set argument naming a time of day.
type timeName string

func (timeName) Type() string { return "TimeSpec" }

func (timeName) Options(cmd.Source) []string { return timeNames() }

// timeSetNamed sets and freezes the world's time at a named time of day.
type timeSetNamed struct {
	Set  cmd.SubCommand `cmd:"set"`
	Time timeName       `cmd:"time"`
}

func (t timeSetNamed) Run(_ cmd.Source, o *cmd.Output, tx *world.Tx) {
	setTime(o, tx, string(t.Time))
}

// timeSetTicks sets and freezes the world's time at a tick count.
type timeSetTicks struct {
	Set   cmd.SubCommand `cmd:"set"`
	Ticks int            `cmd:"ticks"`
}

func (t timeSetTicks) Run(_ cmd.Source, o *cmd.Output, tx *world.Tx) {
	setTime(o, tx, strconv.Itoa(t.Ticks))
}

// setTime freezes tx's world at time spec, which parseTimeOfDay reads.
func setTime(o *cmd.Output, tx *world.Tx, spec string) {
	ticks, err := parseTimeOfDay(spec)
	if err != nil {
		o.Error(err)
		return
	}
	freezeTime(tx.World(), ticks)
	o.Printf("Time set to %d and frozen until /time cycle", ticks)
}

// timeCycle restarts the world's daylight cycle.
type timeCycle struct {
	Cycle cmd.SubCommand `cmd:"cycle"`
}

func (timeCycle) Run(_ cmd.Source, o *cmd.Output, tx *world.Tx) {
	tx.World().StartTime()
	o.Print("Daylight cycle resumed")
}
