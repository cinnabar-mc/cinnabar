package protocol

// packetKey identifies a registered packet.
type packetKey struct {
	state State
	dir   Direction
	id    int32
}

// constructors lists every packet of the protocol. The registry is keyed by what each packet
// reports about itself, so an id lives only in the packet's own ID method.
var constructors = []func() Packet{
	func() Packet { return &Handshake{} },

	func() Packet { return &StatusRequest{} },
	func() Packet { return &StatusPing{} },
	func() Packet { return &StatusResponse{} },
	func() Packet { return &StatusPong{} },

	func() Packet { return &LoginStart{} },
	func() Packet { return &EncryptionResponse{} },
	func() Packet { return &LoginDisconnect{} },
	func() Packet { return &EncryptionRequest{} },
	func() Packet { return &LoginSuccess{} },

	func() Packet { return &KeepAlive{} },
	func() Packet { return &JoinGame{} },
	func() Packet { return &ChatMessage{} },
	func() Packet { return &TimeUpdate{} },
	func() Packet { return &EntityEquipment{} },
	func() Packet { return &SpawnPosition{} },
	func() Packet { return &UpdateHealth{} },
	func() Packet { return &Respawn{} },
	func() Packet { return &PlayerPositionAndLook{} },
	func() Packet { return &HeldItemChange{} },
	func() Packet { return &UseBed{} },
	func() Packet { return &Animation{} },
	func() Packet { return &SpawnPlayer{} },
	func() Packet { return &CollectItem{} },
	func() Packet { return &SpawnObject{} },
	func() Packet { return &SpawnMob{} },
	func() Packet { return &SpawnPainting{} },
	func() Packet { return &SpawnExperienceOrb{} },
	func() Packet { return &EntityVelocity{} },
	func() Packet { return &DestroyEntities{} },
	func() Packet { return &Entity{} },
	func() Packet { return &EntityRelativeMove{} },
	func() Packet { return &EntityLook{} },
	func() Packet { return &EntityLookAndRelativeMove{} },
	func() Packet { return &EntityTeleport{} },
	func() Packet { return &EntityHeadLook{} },
	func() Packet { return &EntityStatus{} },
	func() Packet { return &AttachEntity{} },
	func() Packet { return &EntityMetadata{} },
	func() Packet { return &EntityEffect{} },
	func() Packet { return &RemoveEntityEffect{} },
	func() Packet { return &SetExperience{} },
	func() Packet { return &EntityProperties{} },
	func() Packet { return &ChunkData{} },
	func() Packet { return &MultiBlockChange{} },
	func() Packet { return &BlockChange{} },
	func() Packet { return &BlockAction{} },
	func() Packet { return &BlockBreakAnimation{} },
	func() Packet { return &MapChunkBulk{} },
	func() Packet { return &Explosion{} },
	func() Packet { return &Effect{} },
	func() Packet { return &SoundEffect{} },
	func() Packet { return &Particle{} },
	func() Packet { return &ChangeGameState{} },
	func() Packet { return &SpawnGlobalEntity{} },
	func() Packet { return &OpenWindow{} },
	func() Packet { return &CloseWindow{} },
	func() Packet { return &SetSlot{} },
	func() Packet { return &WindowItems{} },
	func() Packet { return &WindowProperty{} },
	func() Packet { return &ConfirmTransaction{} },
	func() Packet { return &UpdateSign{} },
	func() Packet { return &Maps{} },
	func() Packet { return &UpdateBlockEntity{} },
	func() Packet { return &SignEditorOpen{} },
	func() Packet { return &Statistics{} },
	func() Packet { return &PlayerListItem{} },
	func() Packet { return &PlayerAbilities{} },
	func() Packet { return &TabComplete{} },
	func() Packet { return &ScoreboardObjective{} },
	func() Packet { return &UpdateScore{} },
	func() Packet { return &DisplayScoreboard{} },
	func() Packet { return &Teams{} },
	func() Packet { return &PluginMessage{} },
	func() Packet { return &Disconnect{} },

	func() Packet { return &ClientKeepAlive{} },
	func() Packet { return &ClientChatMessage{} },
	func() Packet { return &ClientUseEntity{} },
	func() Packet { return &ClientPlayer{} },
	func() Packet { return &ClientPlayerPosition{} },
	func() Packet { return &ClientPlayerLook{} },
	func() Packet { return &ClientPlayerPositionLook{} },
	func() Packet { return &ClientPlayerDigging{} },
	func() Packet { return &ClientBlockPlacement{} },
	func() Packet { return &ClientHeldItemChange{} },
	func() Packet { return &ClientAnimation{} },
	func() Packet { return &ClientEntityAction{} },
	func() Packet { return &ClientSteerVehicle{} },
	func() Packet { return &ClientCloseWindow{} },
	func() Packet { return &ClientClickWindow{} },
	func() Packet { return &ClientConfirmTransaction{} },
	func() Packet { return &ClientCreativeInventoryAction{} },
	func() Packet { return &ClientEnchantItem{} },
	func() Packet { return &ClientUpdateSign{} },
	func() Packet { return &ClientPlayerAbilities{} },
	func() Packet { return &ClientTabComplete{} },
	func() Packet { return &ClientSettings{} },
	func() Packet { return &ClientStatus{} },
	func() Packet { return &ClientPluginMessage{} },
}

// registry maps each packet's state, direction and id to its constructor.
var registry = buildRegistry()

// buildRegistry indexes constructors by what each packet reports about itself.
func buildRegistry() map[packetKey]func() Packet {
	index := make(map[packetKey]func() Packet, len(constructors))
	for _, construct := range constructors {
		p := construct()
		index[packetKey{state: p.State(), dir: p.Direction(), id: p.ID()}] = construct
	}
	return index
}

// New returns an empty packet for the id as used in state and travelling in dir. It reports
// false for an id the protocol does not define there.
func New(state State, dir Direction, id int32) (Packet, bool) {
	construct, ok := registry[packetKey{state: state, dir: dir, id: id}]
	if !ok {
		return nil, false
	}
	return construct(), true
}

// Packets returns one empty instance of every packet the protocol defines, in registration
// order.
func Packets() []Packet {
	all := make([]Packet, 0, len(constructors))
	for _, construct := range constructors {
		all = append(all, construct())
	}
	return all
}
