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
// player's own profile.
type skinLoad struct {
	id         uuid.UUID
	key        string
	generation uint64
}

// loadSkin downloads the textures another player's profile names, off the packet path, and
// sends the player's skin once they arrive. Until then, and if they never do, the player
// keeps the default skin. Textures the client already has for the player are not sent again.
func (s *Session) loadSkin(id uuid.UUID, name string, properties []java.ProfileProperty) {
	key := skins.TexturesKey(properties)
	if s.skins == nil || key == "" || s.state.skinKeys[id] == key {
		return
	}
	load := s.startSkinLoad(id, key)
	go s.finishSkinLoad(load, name, func() (skins.Images, error) { return s.skins.Profile(s.lifetime, properties) })
}

// loadOwnSkin downloads the local player's own textures from the session service, as the
// 1.7.10 client does for its own profile.
func (s *Session) loadOwnSkin(id uuid.UUID, name string) {
	if s.skins == nil || s.profileID == "" {
		return
	}
	load := s.startSkinLoad(id, "")
	go s.finishSkinLoad(load, name, func() (skins.Images, error) { return s.skins.Own(s.lifetime, s.profileID) })
}

// startSkinLoad records a new load of key for id, superseding any earlier one; s.mu is held.
func (s *Session) startSkinLoad(id uuid.UUID, key string) skinLoad {
	s.state.skinGeneration++
	load := skinLoad{id: id, key: key, generation: s.state.skinGeneration}
	s.state.skinLoads[id] = load.generation
	return load
}

// finishSkinLoad runs fetch and, if the load is still current, sends the skin to the client.
func (s *Session) finishSkinLoad(load skinLoad, name string, fetch func() (skins.Images, error)) {
	images, err := fetch()
	if s.lifetime.Err() != nil {
		// The session closed; there is no client left to send the skin to.
		return
	}
	if err != nil {
		s.log.Warn("Java Edition player skin not loaded", "player", name, "error", err)
	}
	s.mu.Lock()
	current := s.state.skinLoads[load.id] == load.generation
	if current {
		delete(s.state.skinLoads, load.id)
	}
	apply := current && (images.Skin != nil || images.Cape != nil)
	if apply {
		if load.key != "" {
			s.state.skinKeys[load.id] = load.key
		}
		s.queue(&packet.PlayerSkin{UUID: load.id, Skin: javaSkin(images)})
	}
	s.mu.Unlock()
	if apply {
		s.flushPending()
	}
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

// defaultSkin carries no pixels, so the client draws its own default skin, Steve from the
// installed pack, as the 1.7.10 client draws its steve.png for a player without textures.
// A cape needs skin pixels to be drawn with, so a player wearing the default skin shows none.
func defaultSkin() protocol.Skin {
	return protocol.Skin{SkinID: "java-default", SkinResourcePatch: classicGeometry, Trusted: true}
}
