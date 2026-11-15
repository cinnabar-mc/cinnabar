package translate

import (
	"errors"

	"github.com/google/uuid"
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/skins"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// classicGeometry is the resource patch of a 64x32 skin on the default humanoid model, the
// only player model 1.7.10 has.
var classicGeometry = []byte(`{"geometry":{"default":"geometry.humanoid.custom"}}`)

// maxSkinLoads bounds the skin downloads one session runs at once; later ones wait their turn.
const maxSkinLoads = 4

// queuedSkin is a skin download waiting for one of the session's maxSkinLoads slots.
type queuedSkin struct {
	load  skinLoad
	fetch func() (skins.Images, error)
}

// skinLoad is a skin download under way for a listed player; a newer load or the player
// leaving the list supersedes it. key is the textures key it reads, empty for the local
// player's own profile, and unique the entity unique id a reset list entry names.
type skinLoad struct {
	id         uuid.UUID
	name       string
	key        string
	unique     int64
	generation uint64
}

// loadSkin downloads the textures a spawned player's profile names, off the packet path, and
// sends the player's skin once they arrive. Until then, and if they never do, the player
// keeps the default skin. Textures the client already has for the player are not sent again,
// and a player spawned without textures after wearing some gets the default skin back, as
// the 1.7.10 client gives each spawned player the skin of the profile it came with. s.mu is
// held.
func (s *Session) loadSkin(id uuid.UUID, name string, unique int64, properties []java.ProfileProperty) {
	if s.skins == nil {
		return
	}
	key := skins.TexturesKey(properties)
	sent, wearing := s.state.skinKeys[id]
	if key == "" || (wearing && sent == key) {
		// Nothing to download, and a download for other textures is out of date.
		delete(s.state.skinLoads, id)
		if key == "" && wearing {
			s.resetSkin(id, name, unique)
		}
		return
	}
	s.startSkinLoad(skinLoad{id: id, name: name, key: key, unique: unique}, func() (skins.Images, error) {
		return s.skins.Profile(s.lifetime, properties)
	})
}

// loadOwnSkin downloads the local player's own textures from the session service, as the
// 1.7.10 client does for its own profile; s.mu is held.
func (s *Session) loadOwnSkin(id uuid.UUID, name string) {
	if s.skins == nil || s.profileID == "" {
		return
	}
	s.startSkinLoad(skinLoad{id: id, name: name}, func() (skins.Images, error) {
		return s.skins.Own(s.lifetime, s.profileID)
	})
}

// startSkinLoad records load as the current one for its player, superseding any earlier one,
// and runs fetch for it in a free slot, or queues it until one frees; a newer load for the
// same player replaces a queued one. s.mu is held.
func (s *Session) startSkinLoad(load skinLoad, fetch func() (skins.Images, error)) {
	s.state.skinGeneration++
	load.generation = s.state.skinGeneration
	s.state.skinLoads[load.id] = load.generation
	s.skinWork.Add(1)
	if s.state.skinsRunning < maxSkinLoads {
		s.state.skinsRunning++
		go s.finishSkinLoad(load, fetch)
		return
	}
	for i, queued := range s.state.skinQueue {
		if queued.load.id == load.id {
			s.state.skinQueue = append(s.state.skinQueue[:i], s.state.skinQueue[i+1:]...)
			s.skinWork.Done()
			break
		}
	}
	s.state.skinQueue = append(s.state.skinQueue, queuedSkin{load: load, fetch: fetch})
}

// releaseSkinSlot frees a finished download's slot and starts the next queued download that is
// still current; once the session has closed, it drops the queue. s.mu is held.
func (s *Session) releaseSkinSlot() {
	s.state.skinsRunning--
	for len(s.state.skinQueue) > 0 && s.state.skinsRunning < maxSkinLoads {
		next := s.state.skinQueue[0]
		s.state.skinQueue = s.state.skinQueue[1:]
		if s.lifetime.Err() != nil || s.state.skinLoads[next.load.id] != next.load.generation {
			s.skinWork.Done()
			continue
		}
		s.state.skinsRunning++
		go s.finishSkinLoad(next.load, next.fetch)
	}
}

// finishSkinLoad runs fetch and, if the load is still current, sends the skin to the client.
// A player whose new textures give neither a skin nor a cape gets the default skin back. Only a
// load whose every texture arrived records its textures key, so a later spawn with the same
// key retries a partly failed one.
func (s *Session) finishSkinLoad(load skinLoad, fetch func() (skins.Images, error)) {
	defer s.skinWork.Done()
	images, err := fetch()
	s.mu.Lock()
	defer func() {
		s.releaseSkinSlot()
		s.mu.Unlock()
		s.flushPending()
	}()
	if s.lifetime.Err() != nil {
		// The session closed; there is no client left to send the skin to.
		return
	}
	s.logSkinError(load.name, err)
	current := s.state.skinLoads[load.id] == load.generation
	_, wearing := s.state.skinKeys[load.id]
	switch {
	case !current:
	case images.Skin != nil || images.Cape != nil:
		if load.key != "" {
			// An empty key marks a player wearing textures that did not all arrive.
			s.state.skinKeys[load.id] = ""
			if err == nil {
				s.state.skinKeys[load.id] = load.key
			}
		}
		s.queue(&packet.PlayerSkin{UUID: load.id, Skin: javaSkin(images)})
	case wearing && load.key != "":
		s.resetSkin(load.id, load.name, load.unique)
	}
	if current {
		delete(s.state.skinLoads, load.id)
	}
}

// logSkinError reports why a player's skin did not fully load. Unsigned textures are a
// server's habit rather than a fault, so only the first is a warning and the rest are counted
// at debug level. s.mu is held.
func (s *Session) logSkinError(name string, err error) {
	switch {
	case err == nil:
	case errors.Is(err, skins.ErrUnsigned):
		s.state.unsignedSkins++
		if s.state.unsignedSkins == 1 {
			s.log.Warn("Java Edition server sends skins without a valid signature; those players keep the default skin", "player", name)
		} else {
			s.log.Debug("Java Edition player skin unsigned", "player", name, "count", s.state.unsignedSkins)
		}
	default:
		s.log.Warn("Java Edition player skin not loaded", "player", name, "error", err)
	}
}

// resetSkin lists a player again with the default skin, which replaces the skin it wore: the
// client keeps the old skin when a skin update carries no pixels. s.mu is held.
func (s *Session) resetSkin(id uuid.UUID, name string, unique int64) {
	delete(s.state.skinKeys, id)
	s.queue(&packet.PlayerList{Entries: []protocol.PlayerListEntry{listEntry(id, name, unique, defaultSkin())}})
}

// forgetSkin abandons a player's skin download and forgets its sent textures when it leaves
// the list, since a new entry starts with the default skin; s.mu is held.
func (s *Session) forgetSkin(id uuid.UUID) {
	delete(s.state.skinLoads, id)
	delete(s.state.skinKeys, id)
}

// javaSkin is the Bedrock skin of downloaded textures: a classic 64x32 skin, or the default
// skin when only a cape was downloaded, with the cape when there is one.
func javaSkin(images skins.Images) protocol.Skin {
	skin := defaultSkin()
	if images.Skin != nil {
		skin.SkinID = "java-" + images.SkinHash
		skin.SkinImageWidth, skin.SkinImageHeight = skins.TextureWidth, skins.TextureHeight
		skin.SkinData = images.Skin
	}
	if images.Cape != nil {
		skin.CapeID = images.CapeHash
		skin.CapeImageWidth, skin.CapeImageHeight = skins.TextureWidth, skins.TextureHeight
		skin.CapeData = images.Cape
	}
	skin.FullID = skin.SkinID + skin.CapeID
	return skin
}

// defaultSkin carries no pixels, so the client draws its own default skin: on a Java session,
// the Steve of the installed pack in the 64x32 layout, as the 1.7.10 client draws its steve.png
// for a player without a skin, with any cape the skin carries.
func defaultSkin() protocol.Skin {
	return protocol.Skin{SkinID: "java-default", SkinResourcePatch: classicGeometry, Trusted: true}
}
