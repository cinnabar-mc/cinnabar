package translate

import (
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
func (r *motionReport) next(entityID int32, x, minY, z float64, yaw, pitch float32, onGround, sprinting, sneaking bool) []java.Packet {
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
	stance := minY + eyeHeight
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
		if pk.ActionType == packet.AnimateActionSwingArm {
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
			st.heldSlot = int16(pk.HotBarSlot)
			return s.send(&java.ClientHeldItemChange{Slot: st.heldSlot})
		}
	case *packet.InventoryTransaction:
		switch data := pk.TransactionData.(type) {
		case *protocol.UseItemTransactionData:
			return s.useItem(data)
		case *protocol.UseItemOnEntityTransactionData:
			action := int8(useEntityInteract)
			if data.ActionType == protocol.UseItemOnEntityActionAttack {
				action = useEntityAttack
			}
			return s.send(&java.ClientUseEntity{EntityID: int32(data.TargetEntityRuntimeID), Action: action})
		case *protocol.ReleaseItemTransactionData:
			return s.send(&java.ClientPlayerDigging{Status: digRelease, Face: 255})
		}
	case *packet.PlayerAction:
		if pk.ActionType == protocol.PlayerActionRespawn {
			return s.send(&java.ClientStatus{Action: clientStatusRespawn})
		}
	case *packet.Respawn:
		if pk.State == packet.RespawnStateClientReadyToSpawn {
			return s.send(&java.ClientStatus{Action: clientStatusRespawn})
		}
	case *packet.ContainerClose:
		s.queue(&packet.ContainerClose{WindowID: pk.WindowID, ContainerType: pk.ContainerType})
		return s.send(&java.ClientCloseWindow{WindowID: int8(pk.WindowID)})
	}
	return nil
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
	x, feet, z := float64(pk.Position[0]), float64(pk.Position[1])-networkOffset, float64(pk.Position[2])
	st.position = [3]float64{x, feet, z}
	st.yaw, st.pitch = pk.Yaw, pk.Pitch
	s.publish(x, z)
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
	packets := st.report.next(st.entityID, x, feet, z, pk.Yaw, pk.Pitch, onGround,
		pk.InputData.Load(packet.InputFlagSprinting), pk.InputData.Load(packet.InputFlagSneaking))
	if err := s.send(packets...); err != nil {
		return err
	}
	if actions, ok := pk.BlockActions.Value(); ok {
		for _, action := range actions {
			if err := s.blockAction(action); err != nil {
				return err
			}
		}
	}
	if data, ok := pk.ItemInteractionData.Value(); ok {
		return s.useItem(&data)
	}
	return nil
}

// blockAction translates a mining step to the digging packet a 1.7.10 client sends.
func (s *Session) blockAction(action protocol.PlayerBlockAction) error {
	st := &s.state
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
