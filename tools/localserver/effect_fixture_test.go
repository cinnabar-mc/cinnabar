package main

import (
	"testing"
	"time"
)

func TestFixtureEffectUsesVanillaAmplifierAndInfiniteDurations(t *testing.T) {
	timed, err := fixtureEffect(1, 90, 1)
	if err != nil || timed.Level() != 2 || timed.Duration() != 90*time.Second || timed.Infinite() {
		t.Fatalf("timed speed II = %+v, %v", timed, err)
	}
	infinite, err := fixtureEffect(16, 0, 0)
	if err != nil || !infinite.Infinite() || infinite.Level() != 1 {
		t.Fatalf("infinite night vision = %+v, %v", infinite, err)
	}
	for _, id := range []int{6, 9999} {
		if _, err := fixtureEffect(id, 10, 0); err == nil {
			t.Fatalf("effect %d was accepted", id)
		}
	}
}
