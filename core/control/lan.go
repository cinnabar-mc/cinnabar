package control

import (
	"context"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

// methodLANWorlds lists the worlds found on the local network.
const methodLANWorlds = bridgecontract.MethodLANWorlds

// LANServices lists LAN worlds without an account; a Services value may implement it.
type LANServices interface {
	LANWorlds(ctx context.Context) ([]bridgecontract.LANWorld, error)
}

type lanWorldsResultV1 struct {
	SchemaVersion uint32                    `json:"schema_version"`
	Worlds        []bridgecontract.LANWorld `json:"worlds"`
}

// lanWorldsResult drops trailing worlds until the reply fits one control frame.
func lanWorldsResult(worlds []bridgecontract.LANWorld) lanWorldsResultV1 {
	result := lanWorldsResultV1{SchemaVersion: bridgecontract.ControlSchemaVersion, Worlds: worlds}
	if result.Worlds == nil {
		result.Worlds = []bridgecontract.LANWorld{}
	}
	for len(result.Worlds) > 0 && encodedLen(result) > frameBudget {
		result.Worlds = result.Worlds[:len(result.Worlds)-1]
	}
	return result
}
