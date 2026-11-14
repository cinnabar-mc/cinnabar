package translate

import (
	"crypto/md5"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/google/uuid"
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// entity is a remote entity the server spawned. Position is in the protocol's 1/32-block
// fixed point, which relative moves add to.
type entity struct {
	x, y, z          int32
	yaw, pitch, head int8
	player           bool
	// pendingItem marks a dropped item whose stack has not arrived: Bedrock needs the stack
	// to spawn it.
	pendingItem bool
}

// mobTypes maps 1.7.10 mob type ids to Bedrock actor identifiers.
var mobTypes = map[uint8]string{
	50: "minecraft:creeper", 51: "minecraft:skeleton", 52: "minecraft:spider", 53: "minecraft:zombie",
	54: "minecraft:zombie", 55: "minecraft:slime", 56: "minecraft:ghast", 57: "minecraft:zombie_pigman",
	58: "minecraft:enderman", 59: "minecraft:cave_spider", 60: "minecraft:silverfish", 61: "minecraft:blaze",
	62: "minecraft:magma_cube", 63: "minecraft:ender_dragon", 64: "minecraft:wither", 65: "minecraft:bat",
	66: "minecraft:witch", 90: "minecraft:pig", 91: "minecraft:sheep", 92: "minecraft:cow",
	93: "minecraft:chicken", 94: "minecraft:squid", 95: "minecraft:wolf", 96: "minecraft:mooshroom",
	97: "minecraft:snow_golem", 98: "minecraft:ocelot", 99: "minecraft:iron_golem", 100: "minecraft:horse",
	120: "minecraft:villager",
}

// 1.7.10 object type ids.
const (
	objectBoat = 1
	objectItem = 2
)

// objectTypes maps 1.7.10 object type ids to Bedrock actor identifiers. Dropped items spawn
// through their own packet, and types with no Bedrock actor are left out.
var objectTypes = map[int8]string{
	objectBoat: "minecraft:boat", 10: "minecraft:minecart", 50: "minecraft:tnt", 51: "minecraft:ender_crystal",
	60: "minecraft:arrow", 61: "minecraft:snowball", 62: "minecraft:egg", 63: "minecraft:fireball",
	64: "minecraft:small_fireball", 65: "minecraft:ender_pearl", 66: "minecraft:wither_skull",
	72: "minecraft:eye_of_ender_signal", 73: "minecraft:splash_potion", 75: "minecraft:xp_bottle",
	76: "minecraft:fireworks_rocket", 90: "minecraft:fishing_hook",
}

// Entity metadata indices and flag bits of 1.7.10.
const (
	metadataFlags = 0
	metadataItem  = 10
	flagOnFire    = 1 << 0
	flagSneaking  = 1 << 1
	flagSprinting = 1 << 3
	flagUsingItem = 1 << 4
	flagInvisible = 1 << 5
)

// Java animation and entity status ids.
const (
	animationSwingArm = 0
	animationHurt     = 1
	statusHurt        = 2
	statusDeath       = 3
)

// fixed converts 1/32-block fixed point to blocks.
func fixed(value int32) float32 {
	return float32(value) / 32
}

// degrees converts a rotation byte, 1/256 of a turn, to degrees.
func degrees(value int8) float32 {
	return float32(value) * 360 / 256
}

// velocity converts a velocity in 1/8000 block per tick.
func velocity(x, y, z int16) mgl32.Vec3 {
	return mgl32.Vec3{float32(x) / 8000, float32(y) / 8000, float32(z) / 8000}
}

// nameUUID is the identity the translator gives a player name the server sent without one,
// derived the way offline-mode servers derive theirs.
func nameUUID(name string) uuid.UUID {
	sum := md5.Sum([]byte("OfflinePlayer:" + name))
	sum[6] = sum[6]&0x0f | 0x30
	sum[8] = sum[8]&0x3f | 0x80
	return uuid.UUID(sum)
}

// listEntry is a player-list entry with a placeholder skin: 1.7.10 servers send no skin data.
func listEntry(id uuid.UUID, name string) protocol.PlayerListEntry {
	return protocol.PlayerListEntry{
		ActionType:     protocol.PlayerListActionAdd,
		UUID:           id,
		EntityUniqueID: int64(id.ID()),
		Username:       name,
		Skin:           placeholderSkin(),
	}
}

// placeholderSkin is an opaque 64x32 classic skin on the default geometry.
func placeholderSkin() protocol.Skin {
	const width, height = 64, 32
	pixels := make([]byte, width*height*4)
	for i := 3; i < len(pixels); i += 4 {
		pixels[i-3], pixels[i-2], pixels[i-1], pixels[i] = 0x8a, 0x6a, 0x4f, 0xff
	}
	return protocol.Skin{
		SkinID:            "java-placeholder",
		SkinResourcePatch: []byte(`{"geometry":{"default":"geometry.humanoid.custom"}}`),
		SkinImageWidth:    width,
		SkinImageHeight:   height,
		SkinData:          pixels,
		Trusted:           true,
	}
}

// feet is where an entity stands, which is where Bedrock spawns it.
func (e *entity) feet() mgl32.Vec3 {
	return mgl32.Vec3{fixed(e.x), fixed(e.y), fixed(e.z)}
}

// position is an entity's Bedrock movement position: a player's carries the network offset
// above its feet.
func (e *entity) position() mgl32.Vec3 {
	position := e.feet()
	if e.player {
		position[1] = float32(float64(position[1]) + networkOffset)
	}
	return position
}

// handleEntity translates entity packets; anything else is a packet with no Bedrock
// counterpart yet and is skipped.
func (s *Session) handleEntity(pk java.Packet) error {
	st := &s.state
	switch pk := pk.(type) {
	case *java.SpawnPlayer:
		id, err := uuid.Parse(pk.UUID)
		if err != nil {
			id = nameUUID(pk.Name)
		}
		if _, listed := st.listed[pk.Name]; !listed {
			st.listed[pk.Name] = id
			s.queue(&packet.PlayerList{Entries: []protocol.PlayerListEntry{listEntry(id, pk.Name)}})
		} else {
			id = st.listed[pk.Name]
		}
		s.replace(pk.EntityID)
		spawned := &entity{x: pk.X, y: pk.Y, z: pk.Z, yaw: pk.Yaw, pitch: pk.Pitch, head: pk.Yaw, player: true}
		st.entities[pk.EntityID] = spawned
		s.queue(&packet.AddPlayer{
			UUID:            id,
			Username:        pk.Name,
			EntityRuntimeID: runtimeID(pk.EntityID),
			Position:        spawned.feet(),
			Pitch:           degrees(pk.Pitch),
			Yaw:             degrees(pk.Yaw),
			HeadYaw:         degrees(pk.Yaw),
			EntityMetadata:  entityMetadata(pk.Metadata),
			AbilityData:     protocol.AbilityData{EntityUniqueID: int64(pk.EntityID), Layers: []protocol.AbilityLayer{}},
		})
	case *java.SpawnMob:
		identifier, ok := mobTypes[pk.Type]
		if !ok {
			return nil
		}
		s.replace(pk.EntityID)
		st.entities[pk.EntityID] = &entity{x: pk.X, y: pk.Y, z: pk.Z, yaw: pk.Yaw, pitch: pk.Pitch, head: pk.HeadYaw}
		s.queue(&packet.AddActor{
			EntityUniqueID:  int64(pk.EntityID),
			EntityRuntimeID: runtimeID(pk.EntityID),
			EntityType:      identifier,
			Position:        mgl32.Vec3{fixed(pk.X), fixed(pk.Y), fixed(pk.Z)},
			Velocity:        velocity(pk.VelocityX, pk.VelocityY, pk.VelocityZ),
			Pitch:           degrees(pk.Pitch),
			Yaw:             degrees(pk.Yaw),
			HeadYaw:         degrees(pk.HeadYaw),
			BodyYaw:         degrees(pk.Yaw),
			EntityMetadata:  entityMetadata(pk.Metadata),
		})
	case *java.SpawnObject:
		s.replace(pk.EntityID)
		spawned := &entity{x: pk.X, y: pk.Y, z: pk.Z, yaw: pk.Yaw, pitch: pk.Pitch}
		if pk.Type == objectItem {
			spawned.pendingItem = true
			st.entities[pk.EntityID] = spawned
			return nil
		}
		identifier, ok := objectTypes[pk.Type]
		if !ok {
			return nil
		}
		st.entities[pk.EntityID] = spawned
		actor := &packet.AddActor{
			EntityUniqueID:  int64(pk.EntityID),
			EntityRuntimeID: runtimeID(pk.EntityID),
			EntityType:      identifier,
			Position:        mgl32.Vec3{fixed(pk.X), fixed(pk.Y), fixed(pk.Z)},
			Pitch:           degrees(pk.Pitch),
			Yaw:             degrees(pk.Yaw),
			EntityMetadata:  protocol.NewEntityMetadata(),
		}
		if pk.Data > 0 {
			actor.Velocity = velocity(pk.VelocityX, pk.VelocityY, pk.VelocityZ)
		}
		s.queue(actor)
	case *java.DestroyEntities:
		for _, id := range pk.EntityIDs {
			if _, ok := st.entities[id]; ok {
				delete(st.entities, id)
				s.queue(&packet.RemoveActor{EntityUniqueID: int64(id)})
			}
		}
	case *java.EntityRelativeMove:
		s.moveEntity(pk.EntityID, func(e *entity) { e.x, e.y, e.z = e.x+int32(pk.DX), e.y+int32(pk.DY), e.z+int32(pk.DZ) })
	case *java.EntityLook:
		s.moveEntity(pk.EntityID, func(e *entity) { e.yaw, e.pitch = pk.Yaw, pk.Pitch })
	case *java.EntityLookAndRelativeMove:
		s.moveEntity(pk.EntityID, func(e *entity) {
			e.x, e.y, e.z = e.x+int32(pk.DX), e.y+int32(pk.DY), e.z+int32(pk.DZ)
			e.yaw, e.pitch = pk.Yaw, pk.Pitch
		})
	case *java.EntityTeleport:
		s.moveEntity(pk.EntityID, func(e *entity) { e.x, e.y, e.z, e.yaw, e.pitch = pk.X, pk.Y, pk.Z, pk.Yaw, pk.Pitch })
	case *java.EntityHeadLook:
		s.moveEntity(pk.EntityID, func(e *entity) { e.head = pk.HeadYaw })
	case *java.EntityVelocity:
		// The local player's velocity is the server pushing it: knockback and explosions.
		if pk.EntityID == st.entityID || st.entities[pk.EntityID] != nil {
			s.queue(&packet.SetActorMotion{EntityRuntimeID: runtimeID(pk.EntityID), Velocity: velocity(pk.VelocityX, pk.VelocityY, pk.VelocityZ)})
		}
	case *java.EntityMetadata:
		s.entityMetadata(pk)
	case *java.EntityEquipment:
		s.entityEquipment(pk)
	case *java.Animation:
		if pk.EntityID != st.entityID && st.entities[pk.EntityID] == nil {
			return nil
		}
		switch pk.Animation {
		case animationSwingArm:
			s.queue(&packet.Animate{ActionType: packet.AnimateActionSwingArm, EntityRuntimeID: runtimeID(pk.EntityID)})
		case animationHurt:
			s.queue(&packet.ActorEvent{EntityRuntimeID: runtimeID(pk.EntityID), EventType: packet.ActorEventHurt})
		}
	case *java.EntityStatus:
		if pk.EntityID != st.entityID && st.entities[pk.EntityID] == nil {
			return nil
		}
		switch pk.Status {
		case statusHurt:
			s.queue(&packet.ActorEvent{EntityRuntimeID: runtimeID(pk.EntityID), EventType: packet.ActorEventHurt})
		case statusDeath:
			s.queue(&packet.ActorEvent{EntityRuntimeID: runtimeID(pk.EntityID), EventType: packet.ActorEventDeath})
		}
	case *java.CollectItem:
		if st.entities[pk.CollectedEntityID] != nil {
			s.queue(&packet.TakeItemActor{ItemEntityRuntimeID: runtimeID(pk.CollectedEntityID), TakerEntityRuntimeID: runtimeID(pk.CollectorEntityID)})
		}
	default:
		s.log.Debug("skipping untranslated server packet", "id", pk.ID())
	}
	return nil
}

// replace removes an entity the server spawns again under an id already in use, as the
// 1.7.10 client does before adding the new one.
func (s *Session) replace(id int32) {
	if _, ok := s.state.entities[id]; ok {
		delete(s.state.entities, id)
		s.queue(&packet.RemoveActor{EntityUniqueID: int64(id)})
	}
}

// moveEntity applies a movement to a known entity and sends its new absolute position.
func (s *Session) moveEntity(id int32, apply func(*entity)) {
	moved := s.state.entities[id]
	if moved == nil {
		return
	}
	apply(moved)
	if moved.pendingItem {
		return
	}
	s.queue(&packet.MoveActorAbsolute{
		EntityRuntimeID: runtimeID(id),
		Flags:           packet.MoveFlagOnGround,
		Position:        moved.position(),
		Rotation:        mgl32.Vec3{degrees(moved.pitch), degrees(moved.yaw), degrees(moved.head)},
	})
}

// entityMetadata translates the shared flags byte: burning, sneaking, sprinting, using an
// item and invisibility.
func entityMetadata(metadata java.Metadata) protocol.EntityMetadata {
	translated := protocol.NewEntityMetadata()
	flags, ok := java.MetadataValue[int8](metadata, metadataFlags)
	if !ok {
		return translated
	}
	for bit, flag := range map[int8]uint8{
		flagOnFire: protocol.EntityDataFlagOnFire, flagSneaking: protocol.EntityDataFlagSneaking,
		flagSprinting: protocol.EntityDataFlagSprinting, flagUsingItem: protocol.EntityDataFlagUsingItem,
		flagInvisible: protocol.EntityDataFlagInvisible,
	} {
		if flags&bit != 0 {
			translated.SetFlag(protocol.EntityDataKeyFlags, flag)
		}
	}
	return translated
}

// entityMetadata applies a metadata update: a dropped item spawns once its stack is known,
// and other entities take the new flags.
func (s *Session) entityMetadata(pk *java.EntityMetadata) {
	updated := s.state.entities[pk.EntityID]
	if updated == nil {
		return
	}
	if updated.pendingItem {
		stack, ok := java.MetadataValue[java.ItemStack](pk.Metadata, metadataItem)
		if !ok {
			return
		}
		item := bedrockItem(stack)
		if item.Stack.NetworkID == 0 {
			return
		}
		updated.pendingItem = false
		s.queue(&packet.AddItemActor{
			EntityUniqueID:  int64(pk.EntityID),
			EntityRuntimeID: runtimeID(pk.EntityID),
			Item:            item,
			Position:        updated.position(),
			EntityMetadata:  protocol.NewEntityMetadata(),
		})
		return
	}
	if _, ok := pk.Metadata.Get(metadataFlags); ok {
		s.queue(&packet.SetActorData{EntityRuntimeID: runtimeID(pk.EntityID), EntityMetadata: entityMetadata(pk.Metadata)})
	}
}

// entityEquipment shows the item another entity holds; armour is not translated yet.
func (s *Session) entityEquipment(pk *java.EntityEquipment) {
	if s.state.entities[pk.EntityID] == nil || pk.Slot != 0 {
		return
	}
	s.queue(&packet.MobEquipment{EntityRuntimeID: runtimeID(pk.EntityID), NewItem: bedrockItem(pk.Item)})
}
