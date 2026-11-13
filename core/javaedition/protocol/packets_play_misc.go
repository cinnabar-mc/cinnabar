package protocol

import "math"

// Player state, chat, scoreboard and other packets that are not tied to an entity or a block.

// Field length limits, in UTF-16 units, that 1.7.10 enforces while reading.
const (
	maxObjectiveNameChars    = 16
	maxObjectiveDisplayChars = 32
	maxScoreNameChars        = 16
	maxTeamNameChars         = 16
	maxTeamDisplayChars      = 32
	maxTeamAffixChars        = 16
	maxTeamPlayerChars       = 40
	maxChannelChars          = 20
)

// joinGameHardcoreFlag marks a hardcore world in JoinGame's game mode byte.
const joinGameHardcoreFlag = 0x08

// KeepAlive asks the client to answer with a ClientKeepAlive carrying the same key.
type KeepAlive struct {
	playClientbound
	Key int32
}

func (*KeepAlive) ID() int32 { return 0x00 }

func (p *KeepAlive) Marshal(w *Writer) {
	w.Int32(p.Key)
}

func (p *KeepAlive) Unmarshal(r *Reader) error {
	p.Key = r.Int32()
	return r.Err()
}

// JoinGame starts the play session. GameMode and Hardcore share one byte on the wire.
type JoinGame struct {
	playClientbound
	EntityID   int32
	GameMode   uint8
	Hardcore   bool
	Dimension  int8
	Difficulty uint8
	MaxPlayers uint8
	LevelType  string
}

func (*JoinGame) ID() int32 { return 0x01 }

func (p *JoinGame) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	mode := p.GameMode
	if p.Hardcore {
		mode |= joinGameHardcoreFlag
	}
	w.Uint8(mode)
	w.Int8(p.Dimension)
	w.Uint8(p.Difficulty)
	w.Uint8(p.MaxPlayers)
	w.String(p.LevelType)
}

func (p *JoinGame) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	mode := r.Uint8()
	p.Hardcore = mode&joinGameHardcoreFlag != 0
	p.GameMode = mode &^ joinGameHardcoreFlag
	p.Dimension = r.Int8()
	p.Difficulty = r.Uint8()
	p.MaxPlayers = r.Uint8()
	p.LevelType = r.String(maxLevelTypeChars)
	return r.Err()
}

// ChatMessage shows a chat line. Message is a raw JSON chat component.
type ChatMessage struct {
	playClientbound
	Message string
}

func (*ChatMessage) ID() int32 { return 0x02 }

func (p *ChatMessage) Marshal(w *Writer) {
	w.String(p.Message)
}

func (p *ChatMessage) Unmarshal(r *Reader) error {
	p.Message = r.String(MaxStringChars)
	return r.Err()
}

// UpdateHealth sets the player's health, food and saturation.
type UpdateHealth struct {
	playClientbound
	Health     float32
	Food       int16
	Saturation float32
}

func (*UpdateHealth) ID() int32 { return 0x06 }

func (p *UpdateHealth) Marshal(w *Writer) {
	w.Float32(p.Health)
	w.Int16(p.Food)
	w.Float32(p.Saturation)
}

func (p *UpdateHealth) Unmarshal(r *Reader) error {
	p.Health = r.Float32()
	p.Food = r.Int16()
	p.Saturation = r.Float32()
	return r.Err()
}

// Respawn moves the player to a dimension and resets the world.
type Respawn struct {
	playClientbound
	Dimension  int32
	Difficulty uint8
	GameMode   uint8
	LevelType  string
}

func (*Respawn) ID() int32 { return 0x07 }

func (p *Respawn) Marshal(w *Writer) {
	w.Int32(p.Dimension)
	w.Uint8(p.Difficulty)
	w.Uint8(p.GameMode)
	w.String(p.LevelType)
}

func (p *Respawn) Unmarshal(r *Reader) error {
	p.Dimension = r.Int32()
	p.Difficulty = r.Uint8()
	p.GameMode = r.Uint8()
	p.LevelType = r.String(maxLevelTypeChars)
	return r.Err()
}

// PlayerPositionAndLook places the player. Y is the eye position. The client confirms with a
// ClientPlayerPositionLook.
type PlayerPositionAndLook struct {
	playClientbound
	X        float64
	Y        float64
	Z        float64
	Yaw      float32
	Pitch    float32
	OnGround bool
}

func (*PlayerPositionAndLook) ID() int32 { return 0x08 }

func (p *PlayerPositionAndLook) Marshal(w *Writer) {
	w.Float64(p.X)
	w.Float64(p.Y)
	w.Float64(p.Z)
	w.Float32(p.Yaw)
	w.Float32(p.Pitch)
	w.Bool(p.OnGround)
}

func (p *PlayerPositionAndLook) Unmarshal(r *Reader) error {
	p.X = r.Float64()
	p.Y = r.Float64()
	p.Z = r.Float64()
	p.Yaw = r.Float32()
	p.Pitch = r.Float32()
	p.OnGround = r.Bool()
	return r.Err()
}

// SetExperience sets the experience bar, a fraction from 0 to 1, with the level and total
// points.
type SetExperience struct {
	playClientbound
	Bar             float32
	Level           int16
	TotalExperience int16
}

func (*SetExperience) ID() int32 { return 0x1f }

func (p *SetExperience) Marshal(w *Writer) {
	w.Float32(p.Bar)
	w.Int16(p.Level)
	w.Int16(p.TotalExperience)
}

func (p *SetExperience) Unmarshal(r *Reader) error {
	p.Bar = r.Float32()
	p.Level = r.Int16()
	p.TotalExperience = r.Int16()
	return r.Err()
}

// Statistic is one counter of a Statistics packet.
type Statistic struct {
	Name  string
	Value int32
}

// Statistics reports statistic and achievement counters in wire order.
type Statistics struct {
	playClientbound
	Entries []Statistic
}

func (*Statistics) ID() int32 { return 0x37 }

func (p *Statistics) Marshal(w *Writer) {
	w.VarInt(int32(w.length(len(p.Entries), math.MaxInt32, "statistics")))
	for _, entry := range p.Entries {
		w.String(entry.Name)
		w.VarInt(entry.Value)
	}
}

// Unmarshal reads no entries for a negative count, as 1.7.10 does.
func (p *Statistics) Unmarshal(r *Reader) error {
	count := int(r.VarInt())
	p.Entries = nil
	if count > 0 {
		p.Entries = make([]Statistic, 0, r.capacity(count, 2))
	}
	for i := 0; i < count && r.Err() == nil; i++ {
		p.Entries = append(p.Entries, Statistic{Name: r.String(MaxStringChars), Value: r.VarInt()})
	}
	return r.Err()
}

// PlayerListItem adds, updates or removes a tab list entry.
type PlayerListItem struct {
	playClientbound
	Name   string
	Online bool
	Ping   int16
}

func (*PlayerListItem) ID() int32 { return 0x38 }

func (p *PlayerListItem) Marshal(w *Writer) {
	w.String(p.Name)
	w.Bool(p.Online)
	w.Int16(p.Ping)
}

func (p *PlayerListItem) Unmarshal(r *Reader) error {
	p.Name = r.String(maxUsernameChars)
	p.Online = r.Bool()
	p.Ping = r.Int16()
	return r.Err()
}

// Ability flag bits shared by PlayerAbilities and ClientPlayerAbilities.
const (
	abilityInvulnerable = 1 << iota
	abilityFlying
	abilityAllowFlying
	abilityCreativeMode
)

// abilityFlags packs the four ability booleans into their wire byte.
func abilityFlags(invulnerable, flying, allowFlying, creativeMode bool) uint8 {
	var flags uint8
	if invulnerable {
		flags |= abilityInvulnerable
	}
	if flying {
		flags |= abilityFlying
	}
	if allowFlying {
		flags |= abilityAllowFlying
	}
	if creativeMode {
		flags |= abilityCreativeMode
	}
	return flags
}

// PlayerAbilities sets the player's flight and creative abilities and movement speeds.
type PlayerAbilities struct {
	playClientbound
	Invulnerable bool
	Flying       bool
	AllowFlying  bool
	CreativeMode bool
	FlySpeed     float32
	WalkSpeed    float32
}

func (*PlayerAbilities) ID() int32 { return 0x39 }

func (p *PlayerAbilities) Marshal(w *Writer) {
	w.Uint8(abilityFlags(p.Invulnerable, p.Flying, p.AllowFlying, p.CreativeMode))
	w.Float32(p.FlySpeed)
	w.Float32(p.WalkSpeed)
}

func (p *PlayerAbilities) Unmarshal(r *Reader) error {
	flags := r.Uint8()
	p.Invulnerable = flags&abilityInvulnerable != 0
	p.Flying = flags&abilityFlying != 0
	p.AllowFlying = flags&abilityAllowFlying != 0
	p.CreativeMode = flags&abilityCreativeMode != 0
	p.FlySpeed = r.Float32()
	p.WalkSpeed = r.Float32()
	return r.Err()
}

// TabComplete lists completions for the text of a ClientTabComplete.
type TabComplete struct {
	playClientbound
	Matches []string
}

func (*TabComplete) ID() int32 { return 0x3a }

func (p *TabComplete) Marshal(w *Writer) {
	w.VarInt(int32(w.length(len(p.Matches), math.MaxInt32, "completions")))
	for _, match := range p.Matches {
		w.String(match)
	}
}

func (p *TabComplete) Unmarshal(r *Reader) error {
	count := r.count(int(r.VarInt()), "completions")
	p.Matches = nil
	if count > 0 {
		p.Matches = make([]string, 0, r.capacity(count, 1))
	}
	for i := 0; i < count && r.Err() == nil; i++ {
		p.Matches = append(p.Matches, r.String(MaxStringChars))
	}
	return r.Err()
}

// ScoreboardObjective creates (Mode 0), removes (1) or renames (2) a scoreboard objective.
type ScoreboardObjective struct {
	playClientbound
	Name        string
	DisplayName string
	Mode        int8
}

func (*ScoreboardObjective) ID() int32 { return 0x3b }

func (p *ScoreboardObjective) Marshal(w *Writer) {
	w.String(p.Name)
	w.String(p.DisplayName)
	w.Int8(p.Mode)
}

func (p *ScoreboardObjective) Unmarshal(r *Reader) error {
	p.Name = r.String(maxObjectiveNameChars)
	p.DisplayName = r.String(maxObjectiveDisplayChars)
	p.Mode = r.Int8()
	return r.Err()
}

// updateScoreRemove is the UpdateScore action that carries no objective or value.
const updateScoreRemove = 1

// UpdateScore sets (Action 0) or removes (Action 1) a score. Objective and Value are on the
// wire unless Action is 1.
type UpdateScore struct {
	playClientbound
	ItemName  string
	Action    int8
	Objective string
	Value     int32
}

func (*UpdateScore) ID() int32 { return 0x3c }

func (p *UpdateScore) Marshal(w *Writer) {
	w.String(p.ItemName)
	w.Int8(p.Action)
	if p.Action != updateScoreRemove {
		w.String(p.Objective)
		w.Int32(p.Value)
	}
}

func (p *UpdateScore) Unmarshal(r *Reader) error {
	p.ItemName = r.String(maxScoreNameChars)
	p.Action = r.Int8()
	p.Objective, p.Value = "", 0
	if p.Action != updateScoreRemove {
		p.Objective = r.String(maxObjectiveNameChars)
		p.Value = r.Int32()
	}
	return r.Err()
}

// DisplayScoreboard shows an objective in a display slot; an empty Objective clears the slot.
type DisplayScoreboard struct {
	playClientbound
	Position  int8
	Objective string
}

func (*DisplayScoreboard) ID() int32 { return 0x3d }

func (p *DisplayScoreboard) Marshal(w *Writer) {
	w.Int8(p.Position)
	w.String(p.Objective)
}

func (p *DisplayScoreboard) Unmarshal(r *Reader) error {
	p.Position = r.Int8()
	p.Objective = r.String(maxObjectiveNameChars)
	return r.Err()
}

// Team modes of the Teams packet.
const (
	// TeamCreate creates a team with its settings and members.
	TeamCreate int8 = iota
	// TeamRemove deletes a team.
	TeamRemove
	// TeamUpdate changes a team's settings.
	TeamUpdate
	// TeamAddPlayers adds members to a team.
	TeamAddPlayers
	// TeamRemovePlayers removes members from a team.
	TeamRemovePlayers
)

// Teams creates, removes or changes a scoreboard team. The settings (DisplayName, Prefix,
// Suffix and FriendlyFire) are on the wire for TeamCreate and TeamUpdate, and Players for
// TeamCreate, TeamAddPlayers and TeamRemovePlayers.
type Teams struct {
	playClientbound
	Name         string
	Mode         int8
	DisplayName  string
	Prefix       string
	Suffix       string
	FriendlyFire int8
	Players      []string
}

func (*Teams) ID() int32 { return 0x3e }

// hasSettings reports whether the mode carries the team's settings.
func (p *Teams) hasSettings() bool { return p.Mode == TeamCreate || p.Mode == TeamUpdate }

// hasPlayers reports whether the mode carries a member list.
func (p *Teams) hasPlayers() bool {
	return p.Mode == TeamCreate || p.Mode == TeamAddPlayers || p.Mode == TeamRemovePlayers
}

func (p *Teams) Marshal(w *Writer) {
	w.String(p.Name)
	w.Int8(p.Mode)
	if p.hasSettings() {
		w.String(p.DisplayName)
		w.String(p.Prefix)
		w.String(p.Suffix)
		w.Int8(p.FriendlyFire)
	}
	if p.hasPlayers() {
		w.Int16(int16(w.length(len(p.Players), math.MaxInt16, "team players")))
		for _, player := range p.Players {
			w.String(player)
		}
	}
}

// Unmarshal reads no players for a negative player count, as 1.7.10 does.
func (p *Teams) Unmarshal(r *Reader) error {
	p.Name = r.String(maxTeamNameChars)
	p.Mode = r.Int8()
	p.DisplayName, p.Prefix, p.Suffix, p.FriendlyFire, p.Players = "", "", "", 0, nil
	if p.hasSettings() {
		p.DisplayName = r.String(maxTeamDisplayChars)
		p.Prefix = r.String(maxTeamAffixChars)
		p.Suffix = r.String(maxTeamAffixChars)
		p.FriendlyFire = r.Int8()
	}
	if p.hasPlayers() {
		count := int(r.Int16())
		if count > 0 {
			p.Players = make([]string, 0, r.capacity(count, 1))
		}
		for i := 0; i < count && r.Err() == nil; i++ {
			p.Players = append(p.Players, r.String(maxTeamPlayerChars))
		}
	}
	return r.Err()
}

// PluginMessage carries data on a named plugin channel.
type PluginMessage struct {
	playClientbound
	Channel string
	Data    []byte
}

func (*PluginMessage) ID() int32 { return 0x3f }

func (p *PluginMessage) Marshal(w *Writer) {
	w.String(p.Channel)
	w.Uint16(uint16(w.length(len(p.Data), math.MaxUint16, "plugin message bytes")))
	w.Raw(p.Data)
}

func (p *PluginMessage) Unmarshal(r *Reader) error {
	p.Channel = r.String(maxChannelChars)
	p.Data = r.Bytes(int(r.Uint16()))
	return r.Err()
}

// Disconnect ends the session. Reason is a raw JSON chat component.
type Disconnect struct {
	playClientbound
	Reason string
}

func (*Disconnect) ID() int32 { return 0x40 }

func (p *Disconnect) Marshal(w *Writer) {
	w.String(p.Reason)
}

func (p *Disconnect) Unmarshal(r *Reader) error {
	p.Reason = r.String(MaxStringChars)
	return r.Err()
}
