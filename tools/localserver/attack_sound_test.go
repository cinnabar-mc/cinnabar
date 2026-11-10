package main

import (
	"net"
	"testing"

	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/sound"
	"github.com/go-gl/mathgl/mgl64"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/experience"
)

// addressedViewer is a world viewer reached through one connection address.
type addressedViewer struct {
	world.NopViewer
	addr net.Addr
}

func (v addressedViewer) Addr() net.Addr { return v.addr }

func TestAttackSoundSkipsOnlyTheAttackersConnection(t *testing.T) {
	attacker := addressedViewer{addr: &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1), Port: 50001}}
	other := addressedViewer{addr: &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1), Port: 50002}}
	unaddressed := world.NopViewer{}
	got := othersThan([]world.Viewer{attacker, other, unaddressed}, attacker.addr.String())
	if len(got) != 2 || got[0] != other || got[1] != unaddressed {
		t.Fatalf("attack sound viewers = %v, want the other player and the unaddressed viewer", got)
	}
}

func TestAttackSoundIsReplayedOnlyForTheMarkedAttack(t *testing.T) {
	withPlayer(t, world.GameModeSurvival, func(tx *world.Tx, p *player.Player) {
		installAttackSounds(tx.World())
		play := func(s world.Sound) bool {
			ctx := tx.Event()
			tx.World().Handler().HandleSound(ctx, s, p.Position())
			return ctx.Cancelled()
		}
		if play(sound.Attack{Damage: true}) {
			t.Fatal("an attack sound without an attacker was withheld")
		}
		markAttacker(tx, &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1), Port: 50001})
		if play(sound.Click{}) {
			t.Fatal("a non-attack sound was withheld")
		}
		if !play(sound.Attack{Damage: true}) {
			t.Fatal("the attacker's own attack sound reached every viewer")
		}
		if play(sound.Attack{}) {
			t.Fatal("a later attack sound inherited the consumed mark")
		}
	})
}

func TestTargetDummySpawnsAPlayerAhead(t *testing.T) {
	withPlayer(t, world.GameModeSurvival, func(tx *world.Tx, p *player.Player) {
		runCommand(t, tx, p, "fixture_dummy", "")
		for e := range tx.Players() {
			if dummy := e.(*player.Player); dummy.Name() == "Target" {
				if dummy.Position().Sub(p.Position()).Len() < 1.9 || dummy.MaxHealth() != dummyHealth || !isTargetDummy(dummy) {
					t.Fatalf("dummy at %v with max health %v", dummy.Position(), dummy.MaxHealth())
				}
				return
			}
		}
		t.Fatal("fixture_dummy spawned no player")
	})
}

func TestTargetDummyIsNotKnockedBack(t *testing.T) {
	for _, installed := range []bool{false, true} {
		t.Run(map[bool]string{false: "without_host", true: "with_host"}[installed], func(t *testing.T) {
			withPlayer(t, world.GameModeSurvival, func(tx *world.Tx, p *player.Player) {
				recorder := &entityHookRecorder{}
				if installed {
					p.Handle(&localPlayerHandler{host: recorder})
				}
				runCommand(t, tx, p, "fixture_dummy", "")
				for e := range tx.Players() {
					if dummy := e.(*player.Player); dummy.Name() == "Target" {
						p.AttackEntity(dummy)
						if v := dummy.Velocity(); v.Len() != 0 {
							t.Fatalf("dummy knocked back with velocity %v", v)
						}
						if installed && (len(recorder.attacks) != 1 || !recorder.attacks[0] || recorder.actor != p.H() || recorder.target != dummy.H()) {
							t.Fatalf("dummy attack did not reach the installed host: %+v", recorder)
						}
						return
					}
				}
				t.Fatal("fixture_dummy spawned no player")
			})
		})
	}
}

// TestEveryOpenedWorldWithholdsTheAttackersAttackSound opens a world through the lifecycle that
// Experience-created startup and runtime worlds use, as the default worlds do.
func TestEveryOpenedWorldWithholdsTheAttackersAttackSound(t *testing.T) {
	var lifecycle experience.WorldLifecycle = &pauseState{settings: settings{gameMode: "survival", diff: "peaceful"}}
	w := world.Config{Synchronous: true}.New()
	t.Cleanup(func() { _ = w.Close() })
	lifecycle.WorldOpened(w)
	defer lifecycle.WorldClosed(w)
	if err := w.Do(func(tx *world.Tx) {
		markAttacker(tx, &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1), Port: 50001})
		ctx := tx.Event()
		tx.World().Handler().HandleSound(ctx, sound.Attack{Damage: true}, mgl64.Vec3{})
		if !ctx.Cancelled() {
			t.Error("an opened world sent the attacker its own attack sound")
		}
	}).Wait(t.Context()); err != nil {
		t.Fatal(err)
	}
}
