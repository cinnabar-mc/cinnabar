package translate

import (
	"bytes"
	"image/color"
	"testing"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/skins/skinstest"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const alexUUID = "00000000-0000-4000-8000-000000000009"

// listedSkin returns the skin of the only player-list entry in packets.
func listedSkin(t *testing.T, packets []packet.Packet) protocol.Skin {
	t.Helper()
	for _, pk := range packets {
		if list, ok := pk.(*packet.PlayerList); ok && len(list.Entries) == 1 {
			return list.Entries[0].Skin
		}
	}
	t.Fatalf("no player-list entry in %+v", packets)
	return protocol.Skin{}
}

// nextSkin reads batches until a PlayerSkin arrives.
func (ts *testSession) nextSkin() *packet.PlayerSkin {
	ts.t.Helper()
	for {
		for _, pk := range ts.batch() {
			if skin, ok := pk.(*packet.PlayerSkin); ok {
				return skin
			}
		}
	}
}

// skinLoadsDone reports whether no skin download is under way.
func (ts *testSession) skinLoadsDone() bool {
	ts.session.mu.Lock()
	defer ts.session.mu.Unlock()
	return len(ts.session.state.skinLoads) == 0
}

// checkDefaultSkin fails unless skin carries no pixels, which the client draws as its default.
func checkDefaultSkin(t *testing.T, skin protocol.Skin) {
	t.Helper()
	if len(skin.SkinData) != 0 || len(skin.CapeData) != 0 {
		t.Fatalf("skin has %d pixel bytes and cape %d, want the client's default skin", len(skin.SkinData), len(skin.CapeData))
	}
}

func TestSpawnedPlayersWearTheirSignedSkinAndCape(t *testing.T) {
	services := skinstest.Start(t)
	ts := startTestSessionWith(t, Options{Skins: services.Fetcher()})
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	payload := skinstest.Payload(services.Texture("skin", 64, 32, color.NRGBA{R: 0xff, A: 0xff}), services.Texture("cape", 64, 32, color.NRGBA{B: 0xff, A: 0xff}))
	spawned := ts.serve(&java.SpawnPlayer{EntityID: 9, UUID: alexUUID, Name: "Alex", Properties: []java.ProfileProperty{services.Signed(payload)}})
	checkDefaultSkin(t, listedSkin(t, spawned))

	update := ts.nextSkin()
	if update.UUID != uuid.MustParse(alexUUID) {
		t.Fatalf("skin sent for %v", update.UUID)
	}
	skin := update.Skin
	if skin.SkinImageWidth != 64 || skin.SkinImageHeight != 32 || len(skin.SkinData) != 64*32*4 || skin.SkinData[0+20*64*4] != 0xff {
		t.Fatalf("skin is %dx%d with %d bytes", skin.SkinImageWidth, skin.SkinImageHeight, len(skin.SkinData))
	}
	if skin.CapeImageWidth != 64 || skin.CapeImageHeight != 32 || len(skin.CapeData) != 64*32*4 || skin.CapeID != "cape" {
		t.Fatalf("cape is %dx%d with %d bytes, id %q", skin.CapeImageWidth, skin.CapeImageHeight, len(skin.CapeData), skin.CapeID)
	}
	if !bytes.Equal(skin.SkinResourcePatch, classicGeometry) {
		t.Fatalf("resource patch = %s", skin.SkinResourcePatch)
	}

	// A listed player spawned again already has its skin, so nothing is downloaded or sent.
	ts.serve(&java.DestroyEntities{EntityIDs: []int32{9}})
	ts.serve(&java.SpawnPlayer{EntityID: 9, UUID: alexUUID, Name: "Alex", Properties: []java.ProfileProperty{services.Signed(payload)}})
	if !ts.skinLoadsDone() {
		t.Fatal("a skin the client has was loaded again")
	}

	// A player who left and spawns again is listed with the default skin, then gets the
	// cached one.
	ts.serve(&java.PlayerListItem{Name: "Alex", Online: false})
	relisted := ts.serve(&java.SpawnPlayer{EntityID: 10, UUID: alexUUID, Name: "Alex", Properties: []java.ProfileProperty{services.Signed(payload)}})
	checkDefaultSkin(t, listedSkin(t, relisted))
	if again := ts.nextSkin(); !bytes.Equal(again.Skin.SkinData, skin.SkinData) {
		t.Fatal("the skin sent again differs")
	}
}

func TestPlayersWithoutSignedTexturesKeepTheDefaultSkin(t *testing.T) {
	services := skinstest.Start(t)
	ts := startTestSessionWith(t, Options{Skins: services.Fetcher()})
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})

	// An offline-mode server sends no properties.
	checkDefaultSkin(t, listedSkin(t, ts.serve(&java.SpawnPlayer{EntityID: 9, UUID: alexUUID, Name: "Alex"})))

	unsigned := java.ProfileProperty{Name: "textures", Value: skinstest.Payload(services.Texture("unsigned", 64, 32, color.NRGBA{A: 0xff}), "")}
	checkDefaultSkin(t, listedSkin(t, ts.serve(&java.SpawnPlayer{EntityID: 10, UUID: "00000000-0000-4000-8000-00000000000a", Name: "Notch", Properties: []java.ProfileProperty{unsigned}})))
	testwait.Eventually(t, testwait.DefaultTimeout, "the unsigned skin to be refused", ts.skinLoadsDone)
	for _, pk := range ts.serve(&java.TimeUpdate{WorldAge: 1, TimeOfDay: 1}) {
		if _, ok := pk.(*packet.PlayerSkin); ok {
			t.Fatal("an unsigned skin was sent")
		}
	}
	for _, request := range services.Requests() {
		if request == "https://textures.minecraft.net/texture/unsigned" {
			t.Fatal("an unsigned texture was downloaded")
		}
	}
}

func TestASlowSkinDoesNotHoldBackTranslation(t *testing.T) {
	services := skinstest.Start(t)
	ts := startTestSessionWith(t, Options{Skins: services.Fetcher()})
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	release := services.Block()
	defer release()
	payload := skinstest.Payload(services.Texture("slow", 64, 32, color.NRGBA{G: 0xff, A: 0xff}), "")
	ts.serve(&java.SpawnPlayer{EntityID: 9, UUID: alexUUID, Name: "Alex", Properties: []java.ProfileProperty{services.Signed(payload)}})
	chat := ts.serve(&java.ChatMessage{Message: `{"text":"still here"}`})
	if text, ok := chat[0].(*packet.Text); !ok || text.Message != "still here" {
		t.Fatalf("chat behind a slow skin = %+v", chat)
	}
	release()
	if skin := ts.nextSkin(); skin.Skin.SkinID != "java-slow" {
		t.Fatalf("skin id = %q", skin.Skin.SkinID)
	}
}

func TestTheLocalPlayerWearsItsOwnProfilesSkin(t *testing.T) {
	services := skinstest.Start(t)
	const profile = "0123456789abcdef0123456789abcdef"
	services.Profile(profile, skinstest.Payload(services.Texture("own", 64, 32, color.NRGBA{R: 0xff, A: 0xff}), ""))
	ts := startTestSessionWith(t, Options{Skins: services.Fetcher(), ProfileID: profile})
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	own := ts.serve(&java.PlayerListItem{Name: "Steve", Online: true})
	checkDefaultSkin(t, listedSkin(t, own))
	if skin := ts.nextSkin(); skin.UUID != nameUUID("Steve") || skin.Skin.SkinID != "java-own" {
		t.Fatalf("own skin = %v, %q", skin.UUID, skin.Skin.SkinID)
	}
}
