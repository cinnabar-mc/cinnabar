package main

import (
	"bytes"
	"encoding/json"
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
