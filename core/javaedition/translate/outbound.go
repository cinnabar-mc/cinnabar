package translate

import (
	"math"
	"unicode/utf16"

	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// Java entity action ids.
const (
	actionStartSneaking  = 1
	actionStopSneaking   = 2
	actionStartSprinting = 4
	actionStopSprinting  = 5
)

// Java digging statuses.
const (
	digStart   = 0
	digCancel  = 1
	digFinish  = 2
	digRelease = 5
)

// Java use-entity actions.
const (
	useEntityInteract = 0
	useEntityAttack   = 1
)

// clientStatusRespawn asks the server to respawn the player.
const clientStatusRespawn = 0

// chatLimit is the longest chat message a 1.7.10 server accepts, in UTF-16 units.
const chatLimit = 100

// teleportReach is how close, in blocks, the client's reported position must come to a
// server teleport before its movement is forwarded again.
const teleportReach = 1.0

// motionReport is the 1.7.10 client's memory of what it last reported, which decides the
// packet for each tick: position after moving more than 0.03 blocks or every 20 ticks, look
// whenever either angle changed.
type motionReport struct {
	x, minY, z          float64
	yaw, pitch          float32
	ticksSincePosition  int
	sprinting, sneaking bool
}

// next returns the packets for one client tick and remembers what they report.
// sink is the camera sink below the standing eye height, which the stance, the 1.7.10
// client's posY, leaves out.
func (r *motionReport) next(entityID int32, x, minY, z float64, sink float32, yaw, pitch float32, onGround, sprinting, sneaking bool) []java.Packet {
	var packets []java.Packet
	if sprinting != r.sprinting {
		action := int8(actionStopSprinting)
		if sprinting {
			action = actionStartSprinting
		}
		packets = append(packets, &java.ClientEntityAction{EntityID: entityID, Action: action})
		r.sprinting = sprinting
	}
	if sneaking != r.sneaking {
		action := int8(actionStopSneaking)
		if sneaking {
			action = actionStartSneaking
		}
		packets = append(packets, &java.ClientEntityAction{EntityID: entityID, Action: action})
		r.sneaking = sneaking
	}
	dx, dy, dz := x-r.x, minY-r.minY, z-r.z
	moved := dx*dx+dy*dy+dz*dz > 9.0e-4 || r.ticksSincePosition >= 20
	turned := float64(yaw-r.yaw) != 0 || float64(pitch-r.pitch) != 0
	stance := minY + eyeHeight - float64(sink)
	switch {
	case moved && turned:
		packets = append(packets, &java.ClientPlayerPositionLook{X: x, Y: minY, Stance: stance, Z: z, Yaw: yaw, Pitch: pitch, OnGround: onGround})
	case moved:
		packets = append(packets, &java.ClientPlayerPosition{X: x, Y: minY, Stance: stance, Z: z, OnGround: onGround})
	case turned:
		packets = append(packets, &java.ClientPlayerLook{Yaw: yaw, Pitch: pitch, OnGround: onGround})
	default:
		packets = append(packets, &java.ClientPlayer{OnGround: onGround})
	}
	r.ticksSincePosition++
	if moved {
		r.x, r.minY, r.z, r.ticksSincePosition = x, minY, z, 0
	}
	if turned {
		r.yaw, r.pitch = yaw, pitch
	}
	return packets
}

// handleBedrock translates one client packet. Packets with no Java counterpart are dropped.
func (s *Session) handleBedrock(pk packet.Packet) error {
	st := &s.state
	switch pk := pk.(type) {
	case *packet.NetworkStackLatency:
		return s.answerProbe(pk)
	case *packet.RequestChunkRadius:
		st.radius = min(max(pk.ChunkRadius, 2), 16)
		if !st.joined {
			// StartGame, which must come first, will carry the radius.
			return nil
		}
		s.queue(&packet.ChunkRadiusUpdated{ChunkRadius: st.radius})
		if st.hasPublisher {
			// The published area takes the new radius where the player stands.
			st.hasPublisher = false
			s.publish(st.position[0], st.position[2])
		}
		return s.send(s.clientSettings())
	case *packet.PlayerAuthInput:
		return s.authInput(pk)
	case *packet.Animate:
		if pk.ActionType == packet.AnimateActionSwingArm && swingsOnJava(pk.SwingSource) {
			return s.send(&java.ClientAnimation{EntityID: st.entityID, Animation: 1})
		}
	case *packet.Text:
		if pk.TextType == packet.TextTypeChat {
			return s.chat(pk.Message)
		}
	case *packet.CommandRequest:
		return s.chat(pk.CommandLine)
	case *packet.MobEquipment:
		if pk.WindowID == protocol.WindowIDInventory && pk.HotBarSlot < 9 {
			// The 1.7.10 client reports the slot at its next sync, at the start of a tick or
			// before a click, and only when it changed.
			st.heldSlot = int16(pk.HotBarSlot)
		}
	case *packet.InventoryTransaction:
		switch data := pk.TransactionData.(type) {
		case *protocol.UseItemTransactionData:
			return s.useItem(data)
		case *protocol.UseItemOnEntityTransactionData:
			// PlayerControllerMP.attackEntity and interactWithEntitySendPacket sync the held
			// slot first.
			if err := s.syncHeldSlot(); err != nil {
				return err
			}
			action := int8(useEntityInteract)
			if data.ActionType == protocol.UseItemOnEntityActionAttack {
				action = useEntityAttack
			}
			return s.send(&java.ClientUseEntity{EntityID: javaEntityID(data.TargetEntityRuntimeID), Action: action})
		case *protocol.ReleaseItemTransactionData:
			return s.send(&java.ClientPlayerDigging{Status: digRelease, Face: 255})
		}
	case *packet.PlayerAction:
		if pk.ActionType == protocol.PlayerActionRespawn {
			return s.requestRespawn()
		}
	case *packet.Respawn:
		if pk.State == packet.RespawnStateClientReadyToSpawn {
			return s.requestRespawn()
		}
	case *packet.ContainerClose:
		s.queue(&packet.ContainerClose{WindowID: pk.WindowID, ContainerType: pk.ContainerType})
		return s.send(&java.ClientCloseWindow{WindowID: int8(pk.WindowID)})
	}
	return nil
}

// swingsOnJava reports whether the 1.7.10 client sends a swing for a Bedrock swing of this
// source. It swings for every attack press and after a block it right-clicked, and the
// translator sends its swings while mining itself; it never swings for item uses in the air,
// throws, drops or interactions with entities.
func swingsOnJava(source uint8) bool {
	switch source {
	case packet.AnimateSwingSourceMine, packet.AnimateSwingSourceUseItem, packet.AnimateSwingSourceThrowItem,
		packet.AnimateSwingSourceDropItem, packet.AnimateSwingSourceInteract, packet.AnimateSwingSourceEvent:
		return false
	default:
		return true
	}
}

// syncHeldSlot reports the held slot when it differs from the one last reported, as
// PlayerControllerMP.syncCurrentPlayItem does. A slot the server set is reported back once,
// as the 1.7.10 client does after a held item change.
func (s *Session) syncHeldSlot() error {
	st := &s.state
	if st.heldSlot == st.reportedSlot {
		return nil
	}
	st.reportedSlot = st.heldSlot
	return s.send(&java.ClientHeldItemChange{Slot: st.heldSlot})
}

// requestRespawn asks the server to respawn the dead player once; the Bedrock client asks in
// two packets, the 1.7.10 death screen's button in one.
func (s *Session) requestRespawn() error {
	st := &s.state
	if st.respawnRequested {
		return nil
	}
	st.respawnRequested = true
	return s.send(&java.ClientStatus{Action: clientStatusRespawn})
}

// chat sends a chat message or command, cut to the length a 1.7.10 server accepts. The
// server counts UTF-16 units, so a character outside the basic plane counts twice.
func (s *Session) chat(message string) error {
	units := 0
	for index, character := range message {
		if units += utf16.RuneLen(character); units > chatLimit {
			message = message[:index]
			break
		}
	}
	if message == "" {
		return nil
	}
	return s.send(&java.ClientChatMessage{Message: message})
}

// authInput translates one client tick: sneak and sprint changes, the movement packet,
// block breaking and item use, in that order.
func (s *Session) authInput(pk *packet.PlayerAuthInput) error {
	st := &s.state
	if !st.spawned {
		return nil
	}
	if st.teleport != nil {
		if pk.Position.Sub(*st.teleport).Len() > teleportReach {
			// The client has not applied the teleport yet; its position is stale.
			return nil
		}
		st.teleport = nil
	}
	x, feet, z, exact := exactPosition(pk)
	st.position = [3]float64{x, feet, z}
	yaw := st.look.unwrap(pk.Yaw)
	st.yaw, st.pitch = yaw, pk.Pitch
	s.publish(x, z)
	// Minecraft.runTick first syncs the held slot, then handles the tick's clicks, mining and
	// item use, and only then updates the player, which sends its flight, sprint, sneak and
	// movement packets.
	if err := s.syncHeldSlot(); err != nil {
		return err
	}
	wasBreaking := st.breaking != nil
	if actions, ok := pk.BlockActions.Value(); ok {
		for _, action := range actions {
			if err := s.blockAction(action); err != nil {
				return err
			}
		}
	}
	if wasBreaking || st.breaking != nil {
		// Minecraft.func_147115_a swings every tick the player mines, after damaging the block.
		if err := s.send(&java.ClientAnimation{EntityID: st.entityID, Animation: 1}); err != nil {
			return err
		}
	}
	if data, ok := pk.ItemInteractionData.Value(); ok {
		if err := s.useItem(&data); err != nil {
			return err
		}
	}
	if !st.world.loaded(int32(math.Floor(x)), int32(math.Floor(z))) {
		// EntityClientPlayerMP skips its whole update while its column is not loaded.
		return nil
	}
	// The 1.7.10 client reports a flight toggle before the tick's movement.
	if starts, stops := pk.InputData.Load(packet.InputFlagStartFlying), pk.InputData.Load(packet.InputFlagStopFlying); starts != stops {
		st.abilities.flying = starts
		abilities := st.abilities
		if err := s.send(&java.ClientPlayerAbilities{
			Invulnerable: abilities.invulnerable, Flying: abilities.flying, AllowFlying: abilities.allowFlying,
			CreativeMode: abilities.creative, FlySpeed: abilities.flySpeed, WalkSpeed: abilities.walkSpeed,
		}); err != nil {
			return err
		}
	}
	// Bedrock input has no ground flag. On a Java session the client's physics reports
	// standing on the ground as the vertical collision flag.
	onGround := pk.InputData.Load(packet.InputFlagVerticalCollision)
	// The sprinting flag is the sprint key; the player's sprint state changes only with the
	// start and stop flags, which on a Java session follow the 1.7.10 sprint rules.
	sprinting := st.report.sprinting
	if pk.InputData.Load(packet.InputFlagStartSprinting) {
		sprinting = true
	}
	if pk.InputData.Load(packet.InputFlagStopSprinting) {
		sprinting = false
	}
	packets := st.report.next(st.entityID, x, feet, z, javaSink(pk, exact), yaw, pk.Pitch, onGround,
		sprinting, pk.InputData.Load(packet.InputFlagSneaking))
	return s.send(packets...)
}

// exactPosition returns the feet position a client tick reports. The packet's position is
// in single precision, and a 1.7.10 server reads doubles: a position rounded into a block
// by a hair is one the server will not walk the player out of. On a Java session the client
// therefore sends what the rounding lost in the delta and camera orientation fields, as
// two remainders per axis that add up to the difference. Only a pair shaped like that is
// read as remainders: the second within a single-precision step of the first, and the two
// no larger than rounding can lose. Any other pair is an ordinary delta and orientation. A
// zero orientation component far from the origin, where a step exceeds an ordinary delta,
// still passes; the core's own client sends remainders on every simulated tick of a Java
// session, so that case only arises for ticks it does not simulate.
func exactPosition(pk *packet.PlayerAuthInput) (x, feet, z float64, exact bool) {
	position := [3]float64{float64(pk.Position[0]), float64(pk.Position[1]) - networkOffset, float64(pk.Position[2])}
	exact = true
	for axis := range position {
		first, second := float64(pk.Delta[axis]), float64(pk.CameraOrientation[axis])
		if isRemainderPair(first, second) && math.Abs(first+second) <= roundingBound(pk.Position[axis]) {
			position[axis] += first + second
		} else {
			exact = false
		}
	}
	return position[0], position[1], position[2], exact
}

// maxSink bounds the camera sink a tick may report: the sneak sink plus the most a step can
// add before it decays.
const maxSink = 1.0

// javaSink returns the 1.7.10 camera sink a tick of the client's Java physics reports in its
// analogue move vector. Only such a tick, recognisable by its exact-position remainders,
// carries one; any other tick reports none.
func javaSink(pk *packet.PlayerAuthInput, exact bool) float32 {
	sink := pk.AnalogueMoveVector[0]
	if !exact || pk.AnalogueMoveVector[1] != 0 || !(sink >= 0 && sink <= maxSink) {
		return 0
	}
	return sink
}

// isRemainderPair reports whether second could be what is left of a double after first,
// its nearest single-precision value: at most half a single-precision step of first, which
// is below 2^-24 of first's size, so 2^-23 leaves room for second's own rounding.
func isRemainderPair(first, second float64) bool {
	return math.Abs(second) <= math.Abs(first)*0x1p-23
}

// roundingBound is the most a coordinate sent as value can differ from the double it was
// rounded from: two steps of single precision at its size, which also covers the height's
// second rounding when the network offset is added.
func roundingBound(value float32) float64 {
	size := float32(math.Abs(float64(value))) + 2
	return 2 * float64(math.Nextafter32(size, float32(math.Inf(1)))-size)
}

// blockAction translates a mining step to the digging packet a 1.7.10 client sends.
func (s *Session) blockAction(action protocol.PlayerBlockAction) error {
	st := &s.state
	// PlayerControllerMP syncs the held slot before it digs.
	if err := s.syncHeldSlot(); err != nil {
		return err
	}
	target := [3]int32{action.BlockPos[0], action.BlockPos[1], action.BlockPos[2]}
	dig := func(status uint8, pos [3]int32) error {
		if pos[1] < 0 || pos[1] >= worldHeight {
			return nil
		}
		return s.send(&java.ClientPlayerDigging{Status: status, X: pos[0], Y: uint8(pos[1]), Z: pos[2], Face: uint8(action.Face)})
	}
	switch action.Action {
	case protocol.PlayerActionStartBreak, protocol.PlayerActionCreativePlayerDestroyBlock:
		st.breaking = &target
		return dig(digStart, target)
	case protocol.PlayerActionContinueDestroyBlock:
		if st.breaking != nil && *st.breaking == target {
			return nil
		}
		// The crosshair moved to another block: 1.7.10 cancels the old one and starts anew.
		if st.breaking != nil {
			if err := dig(digCancel, *st.breaking); err != nil {
				return err
			}
		}
		st.breaking = &target
		return dig(digStart, target)
	case protocol.PlayerActionAbortBreak:
		if st.breaking == nil {
			return nil
		}
		previous := *st.breaking
		st.breaking = nil
		return dig(digCancel, previous)
	case protocol.PlayerActionPredictDestroyBlock:
		st.breaking = nil
		return dig(digFinish, target)
	}
	return nil
}

// useItem translates a block click or an item use in the air.
func (s *Session) useItem(data *protocol.UseItemTransactionData) error {
	// PlayerControllerMP syncs the held slot before every use.
	if err := s.syncHeldSlot(); err != nil {
		return err
	}
	switch data.ActionType {
	case protocol.UseItemActionClickBlock:
		y := data.BlockPosition[1]
		if y < 0 || y >= worldHeight {
			return nil
		}
		cursor := func(axis int) uint8 { return uint8(int8(data.ClickedPosition[axis] * 16)) }
		return s.send(&java.ClientBlockPlacement{
			X: data.BlockPosition[0], Y: uint8(y), Z: data.BlockPosition[2], Face: uint8(data.BlockFace),
			HeldItem: s.heldItem(), CursorX: cursor(0), CursorY: cursor(1), CursorZ: cursor(2),
		})
	case protocol.UseItemActionClickAir:
		// A use that targets no block: position -1, 255, -1 and face 255.
		return s.send(&java.ClientBlockPlacement{X: -1, Y: 255, Z: -1, Face: 255, HeldItem: s.heldItem()})
	}
	return nil
}
