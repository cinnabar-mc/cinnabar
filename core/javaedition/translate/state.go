package translate

import (
	"github.com/go-gl/mathgl/mgl32"
	"github.com/google/uuid"
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/sandertv/gophertunnel/minecraft"
)

// eyeHeight is how far the 1.7.10 player position sits above the feet.
const eyeHeight = float64(float32(1.62))

// networkOffset is how far a Bedrock movement position for a standing player sits above the
// feet. It is a little more than the Java eye height, so a position converted with the wrong
// one would put the feet inside the ground.
const networkOffset = float64(float32(1.62001))

// state is everything the translator remembers about the session.
type state struct {
	// gameData is what StartGame announced.
	gameData minecraft.GameData
	// joined is set once the server's join packet has been translated.
	joined bool
	// spawned is set once the client has been told to leave its loading screen.
	spawned bool
	// probes holds the latency probes the client has not answered, by timestamp, and
	// nextProbe the last timestamp given.
	probes    map[int64]probe
	nextProbe int64

	entityID   int32
	dimension  int8
	gameMode   uint8
	difficulty uint8
	world      *world

	// radius is the chunk radius the client asked for, in chunks.
	radius int32
	// publisher is the chunk the last chunk-publisher update was centred on.
	publisher    chunkPos
	hasPublisher bool

	// position is the player's last known position with Y at the feet.
	position   [3]float64
	yaw, pitch float32
	// respawning is set between a respawn in the same dimension and the position after it.
	respawning bool
	// board is the scoreboard the server keeps, of which the Bedrock client shows the
	// displayed objectives.
	board scoreboard
	// teleport is the eye position of a server teleport the client has not reached yet.
	teleport *mgl32.Vec3
	// report decides which movement packet each client tick becomes.
	report motionReport

	entities map[int32]*entity
	// listed maps tab-list names to the identity announced for them.
	listed map[string]uuid.UUID

	health     float32
	food       int16
	saturation float32
	heldSlot   int16
	// inventory is the Java player inventory window as the server last sent it.
	inventory []java.ItemStack
	abilities abilities
	// breaking is the block the client is mining, when it is.
	breaking *[3]int32
}

// abilities are the player abilities the server last sent.
type abilities struct {
	invulnerable, flying, allowFlying, creative bool
	flySpeed, walkSpeed                         float32
}

func newState() state {
	return state{
		probes:    map[int64]probe{},
		world:     newWorld(dimensionOverworld),
		radius:    8,
		entities:  map[int32]*entity{},
		listed:    map[string]uuid.UUID{},
		health:    20,
		food:      20,
		abilities: abilities{flySpeed: 0.05, walkSpeed: 0.1},
		board:     newScoreboard(),
	}
}

// runtimeID is the Bedrock runtime id of a Java entity id. Bedrock reserves id 0, which a
// freshly started 1.7.10 server gives its first entity, so ids move up by one.
func runtimeID(entityID int32) uint64 {
	return uint64(uint32(entityID)) + 1
}

// uniqueID is the Bedrock unique id of a Java entity id, moved up by one like runtimeID.
func uniqueID(entityID int32) int64 {
	return int64(uint32(entityID)) + 1
}

// unlistedUniqueID is the unique id of a listed player whose entity the client has not been
// sent: the 1.7.10 list names no entity. It lies above every id uniqueID gives, so it never
// names another entity.
func unlistedUniqueID(id uuid.UUID) int64 {
	return int64(id.ID()) | 1<<40
}

// javaEntityID is the Java entity id behind a Bedrock runtime id from runtimeID.
func javaEntityID(runtime uint64) int32 {
	return int32(uint32(runtime - 1))
}
