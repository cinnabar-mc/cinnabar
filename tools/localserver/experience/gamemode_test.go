package experience

import (
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/world"
)

// probeGameMode tells its actor's game mode.
const probeGameMode = 39

// flying lets a game mode fly, as the local server's /fly wraps the player's mode.
type flying struct{ world.GameMode }

func (flying) AllowsFlying() bool { return true }

// A game mode is read from what it allows, so each of Dragonfly's reads as itself and a wrapper
// that only adds flight reads as the mode it wraps.
func TestGameModeOfReadsWhatTheModeAllows(t *testing.T) {
	for _, tc := range []struct {
		mode world.GameMode
		want GameMode
	}{
		{world.GameModeSurvival, GameModeSurvival},
		{world.GameModeCreative, GameModeCreative},
		{world.GameModeAdventure, GameModeAdventure},
		{world.GameModeSpectator, GameModeSpectator},
		{flying{world.GameModeSurvival}, GameModeSurvival},
		{flying{world.GameModeAdventure}, GameModeAdventure},
	} {
		if got := gameModeOf(tc.mode); got != tc.want {
			t.Errorf("gameModeOf(%T) = %q, want %q", tc.mode, got, tc.want)
		}
	}
}

// A callback with an actor reads the game mode the actor plays in when it is snapshotted.
func TestCallbackReadsTheActorsGameMode(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	at := probePos(probeGameMode)
	f.place(probeCounter, at, nil)
	f.moveTo(beside(at))
	var want []string
	for _, tc := range []struct {
		mode world.GameMode
		name string
	}{
		{world.GameModeSurvival, "survival"},
		{world.GameModeCreative, "creative"},
		{flying{world.GameModeAdventure}, "adventure"},
		{world.GameModeSpectator, "spectator"},
	} {
		f.do(func(tx *world.Tx) { f.player(tx).SetGameMode(tc.mode) })
		f.activate(at)
		want = append(want, "game mode "+tc.name)
		f.waitTells(5*time.Second, want...)
	}
}
