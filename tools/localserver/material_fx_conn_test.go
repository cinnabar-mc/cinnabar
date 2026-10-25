package main

import (
	"testing"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"

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
