package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"strings"
	"testing"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestMaterialShowcasePackDeclaresSpawnedEntitiesAndBindings(t *testing.T) {
	pack, err := materialShowcasePack()
	if err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{
		"materials/entity.material", "cinnabar/material-bindings.json",
		"entity/material_additive.entity.json", "entity/material_glow.entity.json",
	} {
		data, err := pack.ReadFile(name)
		if err != nil || !json.Valid(data) {
			t.Fatalf("%s: %v", name, err)
		}
	}
	var bindings struct {
		Bindings []struct{ Entity string } `json:"bindings"`
	}
	data, _ := pack.ReadFile("cinnabar/material-bindings.json")
	if err := json.Unmarshal(data, &bindings); err != nil || bindings.Bindings[0].Entity != materialShowcaseGlow {
		t.Fatalf("binding target: %v %+v", err, bindings)
	}
	packets := materialShowcasePackets(mgl32.Vec3{0, 64, 0})
	if len(packets) != len(materialShowcaseActors) {
		t.Fatal("one actor per showcase entry")
	}
	for index, actor := range packets {
		var wire bytes.Buffer
		actor.Marshal(protocol.NewWriter(&wire, 0))
		var decoded packet.AddActor
		decoded.Marshal(protocol.NewReader(&wire, 0, true))
		if decoded.EntityType != materialShowcaseActors[index] || decoded.EntityUniqueID != int64(materialShowcaseFirstID+index) {
			t.Fatalf("actor %d: %+v", index, decoded.EntityType)
		}
	}
	if len(materialShowcaseClear()) != len(materialShowcaseActors) {
		t.Fatal("clearing removes every showcase actor")
	}
}

// TestMaterialShowcaseFilesMatchTheirGenerators keeps the committed geometry equal to the shared
// actor-burst model, and the generated images equal to the digests client tests check theirs
// against.
func TestMaterialShowcaseFilesMatchTheirGenerators(t *testing.T) {
	geometry, err := materialShowcaseFiles.ReadFile(materialShowcaseRoot + "/models/entity/actor_burst.geo.json")
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(geometry, actorBurstGeometry(0)) {
		t.Fatal("the showcase geometry is the actor-burst model")
	}
	pinned, err := os.ReadFile("fixtures/material_showcase.pixels.json")
	if err != nil {
		t.Fatal(err)
	}
	var digests map[string]string
	if err := json.Unmarshal(pinned, &digests); err != nil {
		t.Fatal(err)
	}
	images := materialShowcaseImages()
	if len(digests) != len(images) {
		t.Fatalf("pinned %d images, generated %d", len(digests), len(images))
	}
	for _, image := range images {
		sum := sha256.Sum256(image.rgba().Pix)
		if got := hex.EncodeToString(sum[:]); digests[image.name] != got {
			t.Fatalf("%s pixels digest %s, pinned %s", image.name, got, digests[image.name])
		}
	}
	for _, entity := range []struct{ file, identifier string }{
		{"entity/material_additive.entity.json", materialShowcaseAdditive},
		{"entity/material_glow.entity.json", materialShowcaseGlow},
	} {
		data, err := materialShowcaseFiles.ReadFile(materialShowcaseRoot + "/" + entity.file)
		if err != nil || !strings.Contains(string(data), `"identifier":"`+entity.identifier+`"`) {
			t.Fatalf("%s declares %s: %v", entity.file, entity.identifier, err)
		}
	}
}
