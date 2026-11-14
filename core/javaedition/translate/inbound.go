package translate

import (
	"fmt"
	"time"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/legacy"
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// Game state reasons of the 1.7.10 change-game-state packet.
const (
	gameStateRainBegins = 1
	gameStateRainEnds   = 2
	gameStateGameMode   = 3
)

// handleJava translates one server packet. Packets with no Bedrock counterpart yet are
// skipped; only data that breaks the session's framing is an error.
func (s *Session) handleJava(pk java.Packet) error {
	switch pk := pk.(type) {
	case *java.KeepAlive:
		return s.sendNow(&java.ClientKeepAlive{Key: pk.Key})
	case *java.JoinGame:
		return s.joinGame(pk)
	case *java.Respawn:
		s.respawn(pk)
	case *java.PlayerPositionAndLook:
		return s.positionAndLook(pk)
	case *java.ChunkData:
		return s.chunkData(pk)
	case *java.BlockChange:
		s.blockChange(pk.X, int32(pk.Y), pk.Z, block{id: uint16(pk.BlockID), meta: pk.Metadata})
	case *java.MultiBlockChange:
		for _, record := range pk.Records() {
			x, z := pk.ChunkX<<4+int32(record.X), pk.ChunkZ<<4+int32(record.Z)
			s.blockChange(x, int32(record.Y), z, block{id: record.BlockID, meta: record.Metadata})
		}
	case *java.TimeUpdate:
		timeOfDay := pk.TimeOfDay
		if timeOfDay < 0 {
			// A negative time means the daylight cycle is stopped at its absolute value.
			timeOfDay = -timeOfDay
		}
		s.queue(&packet.SetTime{Time: int32(timeOfDay % 24000)})
	case *java.ChatMessage:
		s.queue(&packet.Text{TextType: packet.TextTypeRaw, Message: chatText(pk.Message)})
	case *java.Disconnect:
		s.queue(&packet.Disconnect{Message: chatText(pk.Reason)})
	case *java.UpdateHealth:
		s.state.health, s.state.food, s.state.saturation = pk.Health, pk.Food, pk.Saturation
		s.queueVitals()
	case *java.SetExperience:
		s.queue(&packet.UpdateAttributes{EntityRuntimeID: runtimeID(s.state.entityID), Attributes: []protocol.Attribute{
			attribute("minecraft:player.level", float32(pk.Level), 0, 24791),
			attribute("minecraft:player.experience", pk.Bar, 0, 1),
		}})
	case *java.PlayerAbilities:
		s.state.abilities = abilities{pk.Invulnerable, pk.Flying, pk.AllowFlying, pk.CreativeMode, pk.FlySpeed, pk.WalkSpeed}
		s.queueAbilities()
	case *java.ChangeGameState:
		s.changeGameState(pk)
	case *java.SpawnPosition:
		s.queue(&packet.SetSpawnPosition{
			SpawnType:     packet.SpawnTypeWorld,
			Position:      protocol.BlockPos{pk.X, pk.Y, pk.Z},
			Dimension:     layoutOf(s.state.dimension).bedrock,
			SpawnPosition: protocol.BlockPos{pk.X, pk.Y, pk.Z},
		})
	case *java.HeldItemChange:
		s.state.heldSlot = int16(pk.Slot)
		s.queue(&packet.PlayerHotBar{SelectedHotBarSlot: uint32(pk.Slot), WindowID: protocol.WindowIDInventory, SelectHotBarSlot: true})
	case *java.PlayerListItem:
		s.playerListItem(pk)
	case *java.SetSlot:
		s.setSlot(pk)
	case *java.WindowItems:
		s.windowItems(pk)
	default:
		return s.handleEntity(pk)
	}
	return nil
}

// sendNow sends Java packets and flushes them: replies the 1.7.10 client makes as soon as it
// handles a packet.
func (s *Session) sendNow(packets ...java.Packet) error {
	if err := s.send(packets...); err != nil {
		return err
	}
	return s.Flush()
}

// joinGame starts the Bedrock session: StartGame and the item registry describe the world,
// and the client is told its settings were received.
func (s *Session) joinGame(pk *java.JoinGame) error {
	st := &s.state
	st.entityID, st.dimension, st.gameMode, st.difficulty = pk.EntityID, pk.Dimension, pk.GameMode, pk.Difficulty
	st.world = newWorld(pk.Dimension)
	st.entities = map[int32]*entity{}
	registry := legacy.Registry()
	items := make([]protocol.ItemEntry, 0, len(registry))
	for _, item := range registry {
		data, err := item.Data()
		if err != nil {
			return err
		}
		version := int32(protocol.ItemEntryVersionLegacy)
		switch item.Version {
		case "data_driven":
			version = protocol.ItemEntryVersionDataDriven
		case "none":
			version = protocol.ItemEntryVersionNone
		}
		items = append(items, protocol.ItemEntry{Name: item.Name, RuntimeID: item.ID, ComponentBased: item.ComponentBased, Version: version, Data: data})
	}
	st.gameData = minecraft.GameData{
		WorldName:               "Java Edition 1.7.10",
		Difficulty:              int32(pk.Difficulty),
		EntityUniqueID:          int64(pk.EntityID),
		EntityRuntimeID:         runtimeID(pk.EntityID),
		PlayerGameMode:          int32(pk.GameMode),
		BaseGameVersion:         "*",
		PlayerPosition:          mgl32.Vec3{0, float32(64 + networkOffset), 0},
		Dimension:               layoutOf(pk.Dimension).bedrock,
		WorldGameMode:           int32(pk.GameMode),
		Hardcore:                pk.Hardcore,
		GameRules:               []protocol.GameRule{{Name: "showcoordinates", Value: true}},
		Items:                   items,
		PlayerMovementSettings:  protocol.PlayerMovementSettings{RewindHistorySize: 40, ServerAuthoritativeBlockBreaking: true},
		PlayerPermissions:       packet.PermissionLevelMember,
		ChunkRadius:             st.radius,
		UseBlockNetworkIDHashes: true,
	}
	start := minecraft.StartGameFromGameData(st.gameData)
	start.Generator = 1
	start.MultiPlayerGame = true
	start.CommandsEnabled = true
	start.GameVersion = protocol.CurrentVersion
	start.MultiPlayerCorrelationID = "java"
	s.queue(start, &packet.ItemRegistry{Items: items})
	st.joined = true
	// The 1.7.10 client sends its settings and brand when it handles the join packet.
	return s.sendNow(s.clientSettings(), &java.ClientPluginMessage{Channel: "MC|Brand", Data: []byte("vanilla")})
}

// clientSettings is the settings packet for the client's chunk radius.
func (s *Session) clientSettings() *java.ClientSettings {
	return &java.ClientSettings{Locale: "en_US", ViewDistance: int8(min(max(s.state.radius, 2), 16)), ChatColors: true, Difficulty: 2, ShowCape: true}
}

// respawn handles a death respawn or a change of dimension. The 1.7.10 client replaces its
// world only when the dimension changes; a respawn in the same dimension keeps the columns
// and entities it has. Either way the player is a new one, which has reported nothing yet.
func (s *Session) respawn(pk *java.Respawn) {
	st := &s.state
	previous := st.dimension
	st.dimension, st.gameMode, st.difficulty = int8(pk.Dimension), pk.GameMode, pk.Difficulty
	st.report, st.breaking = motionReport{}, nil
	if previous != st.dimension {
		// The client respawns through the dimension change; a same-dimension respawn
		// before it that never got a position is over.
		st.respawning = false
		st.world = newWorld(st.dimension)
		st.hasPublisher = false
		for id := range st.entities {
			s.queue(&packet.RemoveActor{EntityUniqueID: int64(id)})
		}
		st.entities = map[int32]*entity{}
		s.queue(&packet.ChangeDimension{
			Dimension: layoutOf(st.dimension).bedrock,
			Position:  mgl32.Vec3{float32(st.position[0]), float32(st.position[1] + networkOffset), float32(st.position[2])},
		})
	} else {
		// The position that follows tells the client where it respawned.
		st.respawning = true
	}
	s.queue(&packet.SetPlayerGameType{GameType: int32(pk.GameMode)})
}

// positionAndLook applies a server teleport: the reply a 1.7.10 client sends goes back at
// once with the packet's own values, and the Bedrock client is moved there.
func (s *Session) positionAndLook(pk *java.PlayerPositionAndLook) error {
	st := &s.state
	feet := pk.Y - eyeHeight
	st.position = [3]float64{pk.X, feet, pk.Z}
	st.yaw, st.pitch = pk.Yaw, pk.Pitch
	eye := mgl32.Vec3{float32(pk.X), float32(feet + networkOffset), float32(pk.Z)}
	st.teleport = &eye
	s.publish(pk.X, pk.Z)
	if st.respawning {
		st.respawning = false
		s.queue(&packet.Respawn{Position: eye, State: packet.RespawnStateReadyToSpawn, EntityRuntimeID: runtimeID(st.entityID)})
	}
	s.queue(&packet.MovePlayer{
		EntityRuntimeID: runtimeID(st.entityID),
		Position:        eye,
		Pitch:           pk.Pitch,
		Yaw:             pk.Yaw,
		HeadYaw:         pk.Yaw,
		Mode:            packet.MoveModeTeleport,
		OnGround:        pk.OnGround,
	})
	if !st.spawned {
		st.spawned = true
		s.queueVitals()
		s.queueAbilities()
		s.queue(&packet.PlayStatus{Status: packet.PlayStatusPlayerSpawn})
	}
	return s.sendNow(&java.ClientPlayerPositionLook{X: pk.X, Y: feet, Stance: pk.Y, Z: pk.Z, Yaw: pk.Yaw, Pitch: pk.Pitch, OnGround: pk.OnGround})
}

// publish tells the client which chunks to keep when the player enters a new chunk.
func (s *Session) publish(x, z float64) {
	st := &s.state
	centre := chunkPos{int32(floor(x)) >> 4, int32(floor(z)) >> 4}
	if st.hasPublisher && st.publisher == centre {
		return
	}
	st.publisher, st.hasPublisher = centre, true
	s.queue(&packet.NetworkChunkPublisherUpdate{
		Position: protocol.BlockPos{int32(floor(x)), int32(floor(st.position[1])), int32(floor(z))},
		Radius:   uint32(st.radius) << 4,
	})
}

// floor rounds toward negative infinity.
func floor(value float64) int64 {
	truncated := int64(value)
	if value < float64(truncated) {
		return truncated - 1
	}
	return truncated
}

// chunkData applies one column: a whole column with no sections unloads it.
func (s *Session) chunkData(pk *java.ChunkData) error {
	pos := chunkPos{pk.ChunkX, pk.ChunkZ}
	if pk.GroundUpContinuous && pk.PrimaryBitmask == 0 {
		s.unload(pos)
		return nil
	}
	data, err := pk.Inflate()
	if err != nil {
		return fmt.Errorf("chunk %d,%d: %w", pk.ChunkX, pk.ChunkZ, err)
	}
	return s.column(pos, data, pk.PrimaryBitmask, pk.AddBitmask, pk.GroundUpContinuous)
}

// mapChunkBulk applies several whole columns, one at a time: each is handed to the client
// before the next is inflated, so a packet of many columns never sits in memory whole. It
// takes the state lock per column rather than across the hand-over, which waits for the
// client when its queue is full.
func (s *Session) mapChunkBulk(pk *java.MapChunkBulk) error {
	err := pk.Each(func(column java.ChunkColumn, data []byte) error {
		s.mu.Lock()
		err := s.column(chunkPos{column.ChunkX, column.ChunkZ}, data, column.PrimaryBitmask, column.AddBitmask, true)
		s.mu.Unlock()
		if err != nil {
			return err
		}
		if !s.flushPending() {
			return s.err()
		}
		return nil
	})
	if err != nil {
		return fmt.Errorf("chunk bulk: %w", err)
	}
	return nil
}

// unload forgets a column the server unloaded and empties it for the client, which has no
// unload of its own inside the published radius.
func (s *Session) unload(pos chunkPos) {
	w := s.state.world
	if w.columns[pos] == nil {
		return
	}
	w.columns[pos] = &column{}
	s.queue(w.levelChunk(pos, s.state.dimension))
	w.unload(pos)
	s.refreshEdges(pos)
}

// column stores a column and sends it to the client, then refreshes the neighbour-dependent
// blocks on the facing edges of the columns around it.
func (s *Session) column(pos chunkPos, data []byte, primary, add uint16, whole bool) error {
	w := s.state.world
	if err := w.fill(pos, data, primary, add, whole); err != nil {
		return fmt.Errorf("chunk %d,%d: %w", pos.x, pos.z, err)
	}
	s.queue(w.levelChunk(pos, s.state.dimension))
	s.refreshEdges(pos)
	w.evict(chunkPos{int32(floor(s.state.position[0])) >> 4, int32(floor(s.state.position[2])) >> 4}, pos)
	return nil
}

// refreshEdges resends the neighbour-dependent blocks on the edges that face a column which
// was loaded, replaced or unloaded: their connections may have changed.
func (s *Session) refreshEdges(pos chunkPos) {
	w := s.state.world
	for _, side := range horizontal {
		neighbour := chunkPos{pos.x + side.dx, pos.z + side.dz}
		if w.columns[neighbour] == nil {
			continue
		}
		for offset := int32(0); offset < 16; offset++ {
			x, z := neighbour.x<<4+offset, neighbour.z<<4+offset
			switch {
			case side.dx > 0:
				x = neighbour.x << 4
			case side.dx < 0:
				x = neighbour.x<<4 + 15
			case side.dz > 0:
				z = neighbour.z << 4
			default:
				z = neighbour.z<<4 + 15
			}
			for y := int32(0); y < worldHeight; y++ {
				if connects(w.block(x, y, z).id) {
					s.queueBlock(x, y, z)
				}
			}
		}
	}
}

// blockChange stores a block and updates the client, along with neighbours whose
// connections the change can alter.
func (s *Session) blockChange(x, y, z int32, value block) {
	w := s.state.world
	if !w.set(x, y, z, value) {
		return
	}
	s.queueBlock(x, y, z)
	for _, side := range horizontal {
		if connects(w.block(x+side.dx, y, z+side.dz).id) {
			s.queueBlock(x+side.dx, y, z+side.dz)
		}
	}
}

// queueBlock sends the client the current state of one block.
func (s *Session) queueBlock(x, y, z int32) {
	s.queue(&packet.UpdateBlock{
		Position:          protocol.BlockPos{x, y, z},
		NewBlockRuntimeID: s.state.world.networkID(x, y, z),
		Flags:             packet.BlockUpdateNetwork,
	})
}

// changeGameState translates weather and game mode changes.
func (s *Session) changeGameState(pk *java.ChangeGameState) {
	switch pk.Reason {
	case gameStateRainBegins:
		s.queue(&packet.LevelEvent{EventType: packet.LevelEventStartRaining, EventData: 65535})
	case gameStateRainEnds:
		s.queue(&packet.LevelEvent{EventType: packet.LevelEventStopRaining})
	case gameStateGameMode:
		s.state.gameMode = uint8(pk.Value)
		s.queue(&packet.SetPlayerGameType{GameType: int32(pk.Value)})
	}
}

// attribute builds a Bedrock attribute with its range.
func attribute(name string, value, minimum, maximum float32) protocol.Attribute {
	return protocol.Attribute{
		AttributeValue: protocol.AttributeValue{Name: name, Value: value, Min: minimum, Max: maximum},
		DefaultMin:     minimum,
		DefaultMax:     maximum,
		Default:        maximum,
	}
}

// queueVitals sends the player's health, hunger and saturation.
func (s *Session) queueVitals() {
	st := &s.state
	s.queue(&packet.UpdateAttributes{EntityRuntimeID: runtimeID(st.entityID), Attributes: []protocol.Attribute{
		attribute("minecraft:health", max(st.health, 0), 0, 20),
		attribute("minecraft:player.hunger", float32(st.food), 0, 20),
		attribute("minecraft:player.saturation", st.saturation, 0, 20),
	}})
}

// queueAbilities sends the player's abilities as one base layer.
func (s *Session) queueAbilities() {
	st := &s.state
	granted := uint32(protocol.AbilityBuild | protocol.AbilityMine | protocol.AbilityDoorsAndSwitches | protocol.AbilityOpenContainers | protocol.AbilityAttackPlayers | protocol.AbilityAttackMobs)
	set := func(flag uint32, on bool) {
		if on {
			granted |= flag
		}
	}
	set(protocol.AbilityInvulnerable, st.abilities.invulnerable)
	set(protocol.AbilityFlying, st.abilities.flying)
	set(protocol.AbilityMayFly, st.abilities.allowFlying)
	set(protocol.AbilityInstantBuild, st.abilities.creative)
	s.queue(&packet.UpdateAbilities{AbilityData: protocol.AbilityData{
		EntityUniqueID:     int64(st.entityID),
		PlayerPermissions:  packet.PermissionLevelMember,
		CommandPermissions: protocol.CommandPermissionLevelAny,
		Layers: []protocol.AbilityLayer{{
			Type:             protocol.AbilityLayerTypeBase,
			Abilities:        protocol.AbilityCount - 1,
			Values:           granted,
			FlySpeed:         st.abilities.flySpeed,
			VerticalFlySpeed: 1,
			WalkSpeed:        st.abilities.walkSpeed,
		}},
	}})
}

// playerListItem keeps the tab list: names are added with an offline-style identity, and the
// server's ping for the local player becomes the session latency.
func (s *Session) playerListItem(pk *java.PlayerListItem) {
	st := &s.state
	if pk.Name == s.conn.Profile().Username && pk.Online {
		s.latency.Store(int64(time.Duration(pk.Ping) * time.Millisecond / 2))
	}
	id, known := st.listed[pk.Name]
	switch {
	case pk.Online && !known:
		id = nameUUID(pk.Name)
		st.listed[pk.Name] = id
		s.queue(&packet.PlayerList{Entries: []protocol.PlayerListEntry{listEntry(id, pk.Name)}})
	case !pk.Online && known:
		delete(st.listed, pk.Name)
		s.queue(&packet.PlayerList{Entries: []protocol.PlayerListEntry{{ActionType: protocol.PlayerListActionRemove, UUID: id}}})
	}
}
