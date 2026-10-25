package main

import (
	"fmt"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
)

// materialFxBindPulses is how many bind records a spawn sends, materialFxBindGap apart: the
// client part declares a few materials per callback, so the gallery fills in over several.
const (
	materialFxBindPulses = 7
	materialFxBindGap    = 150 * time.Millisecond
	// materialFxRadarGap is how often the screen's radar gets the players' positions.
	materialFxRadarGap = 500 * time.Millisecond
	// materialFxRadarPlayers is how many players the radar shows.
	materialFxRadarPlayers = 4
)

// materialFxPlayers are the positions of every connected player, for the radar.
type materialFxPlayers struct {
	mu        sync.Mutex
	positions map[uuid.UUID]mgl32.Vec3
}

// set records `id`'s feet, or forgets the player when `gone`.
func (p *materialFxPlayers) set(id uuid.UUID, feet mgl32.Vec3, gone bool) {
	p.mu.Lock()
	defer p.mu.Unlock()
	if gone {
		delete(p.positions, id)
		return
	}
	p.positions[id] = feet
}

// snapshot is up to `limit` positions, `first`'s first.
func (p *materialFxPlayers) snapshot(first uuid.UUID, limit int) []mgl32.Vec3 {
	p.mu.Lock()
	defer p.mu.Unlock()
	out := make([]mgl32.Vec3, 0, limit)
	if feet, ok := p.positions[first]; ok {
		out = append(out, feet)
	}
	for id, feet := range p.positions {
		if id != first && len(out) < limit {
			out = append(out, feet)
		}
	}
	return out
}

// materialFxListener adds /materialfx to opted-in local worlds.
func materialFxListener(inner func(server.Config) (server.Listener, error), players func(uuid.UUID) (*world.EntityHandle, bool), ext *experienceSender) func(server.Config) (server.Listener, error) {
	everyone := &materialFxPlayers{positions: map[uuid.UUID]mgl32.Vec3{}}
	return func(conf server.Config) (server.Listener, error) {
		listener, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &materialFxPacketListener{Listener: listener, players: players, ext: ext, everyone: everyone}, nil
	}
}

// experienceSender is the client part channel the commands drive; nil without -extension flags.
type experienceSender struct {
	send func(player uuid.UUID, exp, channel string, schema uint16, payload []cinnabarext.Scalar) bool
}

type materialFxPacketListener struct {
	server.Listener
	players  func(uuid.UUID) (*world.EntityHandle, bool)
	ext      *experienceSender
	everyone *materialFxPlayers
}

// Accept tracks each connection's player and position for the showcase command.
func (l *materialFxPacketListener) Accept() (session.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	id, _ := uuid.Parse(conn.IdentityData().Identity)
	return &materialFxConn{Conn: conn, players: l.players, ext: l.ext, everyone: l.everyone, id: id, done: make(chan struct{})}, nil
}

// Disconnect unwraps this fixture before delegating to the original listener.
func (l *materialFxPacketListener) Disconnect(conn session.Conn, reason string) error {
	if wrapped, ok := conn.(*materialFxConn); ok {
		conn = wrapped.Conn
	}
	return l.Listener.Disconnect(conn, reason)
}

type materialFxConn struct {
	session.Conn
	players  func(uuid.UUID) (*world.EntityHandle, bool)
	ext      *experienceSender
	everyone *materialFxPlayers
	id       uuid.UUID
	view     showcaseViewpoint
	done     chan struct{}
	closing  sync.Once

	mu sync.Mutex
	// gallery is the viewpoint the gallery was built from, once it is.
	gallery *showcaseViewpoint
	thermal bool
	// portalDistance is the distance last sent, in thousandths.
	portalDistance int64
	// missing is set while the client part refuses records, so the player hears of it once.
	missing bool
	// outOfRange is set once the player has heard that a record could not be carried.
	outOfRange bool
}

// ReadPacket runs /materialfx, follows the player for the radar and the portal, and turns hits
// on the liquid metal mob into ripples and shockwaves.
func (c *materialFxConn) ReadPacket() (packet.Packet, error) {
	for {
		pk, err := c.Conn.ReadPacket()
		if err != nil {
			c.close()
			return pk, err
		}
		switch pk := pk.(type) {
		case *packet.PlayerAuthInput:
			c.view = viewpointOf(pk)
			c.everyone.set(c.id, c.view.feet, false)
			c.followPortal()
		case *packet.InventoryTransaction:
			if use, ok := pk.TransactionData.(*protocol.UseItemOnEntityTransactionData); ok && use.ActionType == protocol.UseItemOnEntityActionAttack {
				c.hit(use)
			}
		case *packet.CommandRequest:
			fields := strings.Fields(strings.TrimPrefix(strings.TrimSpace(pk.CommandLine), "/"))
			if len(fields) > 0 && fields[0] == "materialfx" {
				if err := c.run(parseMaterialFx(fields[1:])); err != nil {
					c.close()
					return nil, err
				}
				continue
			}
		}
		return pk, nil
	}
}

// close stops the radar and forgets the player's position, once, when the connection ends.
func (c *materialFxConn) close() {
	c.closing.Do(func() {
		close(c.done)
		c.everyone.set(c.id, mgl32.Vec3{}, true)
	})
}

// send hands `records` to the client part, each on its own, so one that cannot go does not hold
// back the rest. A record with an argument the control channel cannot carry is never sent, and
// the player hears of it once. When the client part refuses records (it is not running, or they
// came faster than its rate allows), the player is told once, with how many of the batch were
// refused, until a record goes through again.
func (c *materialFxConn) send(records ...[]cinnabarext.Scalar) {
	refused := 0
	for _, record := range records {
		if !materialFxCarried(record) {
			c.mu.Lock()
			warn := !c.outOfRange
			c.outOfRange = true
			c.mu.Unlock()
			if warn {
				c.tell(fmt.Sprintf("materialfx: skipped a record beyond the control channel's ±%d blocks", materialFxArgLimit/1000))
			}
			continue
		}
		if c.ext == nil || !c.ext.send(c.id, materialFxID, materialFxChannel, materialFxSchema, record) {
			refused++
			continue
		}
		c.mu.Lock()
		c.missing = false
		c.mu.Unlock()
	}
	if refused == 0 {
		return
	}
	c.mu.Lock()
	warn := !c.missing
	c.missing = true
	c.mu.Unlock()
	if warn {
		c.tell(fmt.Sprintf("materialfx: the client part refused %d of %d records: it is not active (run the client with CINNABAR_DEV_SERVER_EXPERIENCES=1) or they came too fast", refused, len(records)))
	}
}

// run carries out one parsed command on this connection.
func (c *materialFxConn) run(command materialFxCommand) error {
	if command.usage != "" {
		c.tell(command.usage)
		return nil
	}
	c.mu.Lock()
	gallery := c.gallery
	c.mu.Unlock()
	switch {
	case command.spawn:
		return c.spawn()
	case gallery == nil && (command.boom || command.corrupt || command.cleanse):
		c.tell("materialfx: run /materialfx first to build the gallery")
	case command.boom:
		arena := materialFxPieces(*gallery)[materialFxArenaPiece]
		c.send(materialFxRecord(materialFxBoom, milli(arena[0]), milli(arena[2]), materialFxNowMs(time.Now())))
	case command.thermal:
		c.mu.Lock()
		c.thermal = !c.thermal
		on := c.thermal
		c.mu.Unlock()
		value := int64(0)
		if on {
			value = 1
		}
		c.send(materialFxRecord(materialFxThermal, value))
	case command.corrupt:
		source := materialFxCorruptionSource(*gallery)
		c.send(materialFxRecord(materialFxCorrupt, milli(source[0]), milli(source[1]), milli(source[2]), materialFxNowMs(time.Now())))
	case command.cleanse:
		c.send(materialFxRecord(materialFxCorrupt, 0, 0, 0, -1))
	case command.heartbeat != nil:
		c.send(materialFxRecord(materialFxHeartbeat, *command.heartbeat, materialFxNowMs(time.Now())))
	}
	c.send(command.controls...)
	return nil
}

// spawn builds the gallery in front of the player, sends its layout and binds its materials
// over several callbacks, and starts the radar.
func (c *materialFxConn) spawn() error {
	view := c.view
	c.mu.Lock()
	previous := c.gallery
	first := previous == nil
	c.gallery = &view
	c.mu.Unlock()
	var packets []packet.Packet
	if !first {
		packets = materialFxClear()
	}
	for _, fixture := range append(packets, materialFxActors(view)...) {
		if err := c.Conn.WritePacket(fixture); err != nil {
			return err
		}
	}
	c.placeBlocks(previous, view)
	c.send(materialFxLayoutRecords(view)...)
	for pulse := range materialFxBindPulses {
		time.AfterFunc(time.Duration(pulse)*materialFxBindGap, func() { c.send(materialFxRecord(materialFxBind)) })
	}
	if first {
		go c.radar()
	}
	return nil
}

// radar sends the players' positions to the screen until the connection closes.
func (c *materialFxConn) radar() {
	ticker := time.NewTicker(materialFxRadarGap)
	defer ticker.Stop()
	for {
		select {
		case <-c.done:
			return
		case <-ticker.C:
			// Players the channel cannot place are left off, so the others keep their slots.
			positions := slices.DeleteFunc(c.everyone.snapshot(c.id, materialFxRadarPlayers), func(feet mgl32.Vec3) bool {
				return !materialFxCarried(materialFxRecord(materialFxRadar, 0, milli(feet[0]), milli(feet[2])))
			})
			for index, feet := range positions {
				c.send(materialFxRecord(materialFxRadar, int64(index), milli(feet[0]), milli(feet[2]), int64(len(positions))))
			}
		}
	}
}

// followPortal sends the player's distance to the portal, and when they were there, whenever it
// changed by a quarter block; the client part keeps the portal's turn continuous across it.
// It sends on the reading goroutine, after releasing c.mu, so updates arrive in input order.
func (c *materialFxConn) followPortal() {
	c.mu.Lock()
	if c.gallery == nil || c.ext == nil {
		c.mu.Unlock()
		return
	}
	portal := materialFxPieces(*c.gallery)[materialFxPortalPiece]
	distance := milli(portal.Sub(c.view.feet.Add(mgl32.Vec3{0, actorBurstEyeHeight, 0})).Len())
	if abs64(distance-c.portalDistance) < 250 {
		c.mu.Unlock()
		return
	}
	c.portalDistance = distance
	c.mu.Unlock()
	c.send(materialFxRecord(materialFxPortalDistance, distance, materialFxNowMs(time.Now())))
}

// hit ripples the liquid metal mob where it was struck and sends a shockwave across the arena
// from its feet.
func (c *materialFxConn) hit(use *protocol.UseItemOnEntityTransactionData) {
	c.mu.Lock()
	gallery := c.gallery
	c.mu.Unlock()
	if gallery == nil {
		return
	}
	for index, entity := range materialFxEntities {
		if entity.id != materialFxMetal || use.TargetEntityRuntimeID != uint64(materialFxFirstID+index) {
			continue
		}
		feet := materialFxMobFeet(*gallery, index)
		struck := feet.Add(use.ClickedPosition)
		now := materialFxNowMs(time.Now())
		c.send(
			materialFxRecord(materialFxHit, milli(struck[0]), milli(struck[1]), milli(struck[2]), now),
			materialFxRecord(materialFxBoom, milli(feet[0]), milli(feet[2]), now),
		)
	}
}

// abs64 is |v|.
func abs64(v int64) int64 {
	if v < 0 {
		return -v
	}
	return v
}

// do runs `f` on the connection's player in its world, if it is still there.
func (c *materialFxConn) do(f func(tx *world.Tx, p *player.Player)) {
	handle, ok := c.players(c.id)
	if !ok {
		return
	}
	handle.Do(func(tx *world.Tx, e world.Entity) {
		if p, ok := e.(*player.Player); ok {
			f(tx, p)
		}
	})
}

// tell shows `text` to the connection's player.
func (c *materialFxConn) tell(text string) {
	c.do(func(_ *world.Tx, p *player.Player) { p.Message(text) })
}

// placeBlocks builds the gallery's blocks in front of `view`, when the Experience registered
// them, and says so when it did not. The blocks of the gallery built from `previous`, if any,
// that are still in place and not part of the new one become air.
func (c *materialFxConn) placeBlocks(previous *showcaseViewpoint, view showcaseViewpoint) {
	c.do(func(tx *world.Tx, p *player.Player) {
		blocks := materialFxBlocks(view)
		if previous != nil {
			for pos, name := range materialFxBlocks(*previous) {
				if _, kept := blocks[pos]; kept {
					continue
				}
				if current, _ := tx.Block(pos).EncodeBlock(); current == name {
					tx.SetBlock(pos, block.Air{}, nil)
				}
			}
		}
		registry := tx.World().BlockRegistry()
		for pos, name := range blocks {
			b, ok := registry.BlockByName(name, nil)
			if !ok {
				p.Message("materialfx: the Experience's blocks are not registered (start the server with -experiences)")
				return
			}
			tx.SetBlock(pos, b, nil)
		}
	})
}
