package translate

import (
	"github.com/google/uuid"
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/skins"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// classicGeometry is the resource patch of a 64x32 skin on the default humanoid model, the
// only player model 1.7.10 has.
var classicGeometry = []byte(`{"geometry":{"default":"geometry.humanoid.custom"}}`)

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
	load := s.startSkinLoad(skinLoad{id: id, name: name, key: key, unique: unique})
	go s.finishSkinLoad(load, func() (skins.Images, error) { return s.skins.Profile(s.lifetime, properties) })
}

// loadOwnSkin downloads the local player's own textures from the session service, as the
// 1.7.10 client does for its own profile; s.mu is held.
func (s *Session) loadOwnSkin(id uuid.UUID, name string) {
	if s.skins == nil || s.profileID == "" {
		return
	}
	load := s.startSkinLoad(skinLoad{id: id, name: name})
	go s.finishSkinLoad(load, func() (skins.Images, error) { return s.skins.Own(s.lifetime, s.profileID) })
}

// startSkinLoad records load as the current one for its player, superseding any earlier one;
// s.mu is held. The caller runs finishSkinLoad for it.
func (s *Session) startSkinLoad(load skinLoad) skinLoad {
	s.state.skinGeneration++
	load.generation = s.state.skinGeneration
	s.state.skinLoads[load.id] = load.generation
	s.skinWork.Add(1)
	return load
}

// finishSkinLoad runs fetch and, if the load is still current, sends the skin to the client.
// A player whose new textures give no skin gets the default skin back; a cape alone cannot be
// drawn without skin pixels, so it is dropped.
func (s *Session) finishSkinLoad(load skinLoad, fetch func() (skins.Images, error)) {
	defer s.skinWork.Done()
	images, err := fetch()
	if s.lifetime.Err() != nil {
		// The session closed; there is no client left to send the skin to.
		return
	}
	if err != nil {
		s.log.Warn("Java Edition player skin not loaded", "player", load.name, "error", err)
	}
	s.mu.Lock()
	current := s.state.skinLoads[load.id] == load.generation
	_, wearing := s.state.skinKeys[load.id]
	switch {
	case !current:
	case images.Skin != nil:
		if load.key != "" {
			s.state.skinKeys[load.id] = load.key
		}
		s.queue(&packet.PlayerSkin{UUID: load.id, Skin: javaSkin(images)})
	case wearing && load.key != "":
		s.resetSkin(load.id, load.name, load.unique)
	}
	if current {
		delete(s.state.skinLoads, load.id)
	}
	s.mu.Unlock()
	s.flushPending()
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

// javaSkin is the Bedrock skin of downloaded textures, which hold a skin: a classic 64x32
// skin, with the cape when there is one.
func javaSkin(images skins.Images) protocol.Skin {
	skin := defaultSkin()
	skin.SkinID = "java-" + images.SkinHash
	skin.SkinImageWidth, skin.SkinImageHeight = skins.TextureWidth, skins.TextureHeight
	skin.SkinData = images.Skin
	if images.Cape != nil {
		skin.CapeID = images.CapeHash
		skin.CapeImageWidth, skin.CapeImageHeight = skins.TextureWidth, skins.TextureHeight
		skin.CapeData = images.Cape
	}
	skin.FullID = skin.SkinID + skin.CapeID
	return skin
}

// defaultSkin carries no pixels, so the client draws its own default skin, Steve from the
// installed pack, as the 1.7.10 client draws its steve.png for a player without textures.
// A cape needs skin pixels to be drawn with, so a player wearing the default skin shows none.
func defaultSkin() protocol.Skin {
	return protocol.Skin{SkinID: "java-default", SkinResourcePatch: classicGeometry, Trusted: true}
}
