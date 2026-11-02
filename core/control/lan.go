package control

import (
	"context"
	"encoding/json"
	"sync"
	"unicode/utf8"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

// methodLANWorlds lists the worlds found on the local network.
const methodLANWorlds = bridgecontract.MethodLANWorlds

// LANServices lists LAN worlds without an account; a Services value may implement it.
type LANServices interface {
	LANWorlds(ctx context.Context) ([]bridgecontract.LANWorld, error)
}

// lanWorldsParamsV1 asks for a later page of an earlier listing; omitted, it starts a new one.
type lanWorldsParamsV1 struct {
	Snapshot uint64 `json:"snapshot"`
	Offset   int    `json:"offset"`
}

type lanWorldsResultV1 struct {
	SchemaVersion uint32                    `json:"schema_version"`
	Snapshot      uint64                    `json:"snapshot"`
	Worlds        []bridgecontract.LANWorld `json:"worlds"`
	// Next is the offset of the following page, omitted on the last one.
	Next int `json:"next,omitempty"`
}

// lanListings keeps the latest listing so its later pages come from the same snapshot.
type lanListings struct {
	mu     sync.Mutex
	id     uint64
	worlds []bridgecontract.LANWorld
}

// page returns the first page of a fresh listing, or a later page of the latest one; ok is false
// for a page of a listing that has since been replaced.
func (l *lanListings) page(ctx context.Context, lan LANServices, params *lanWorldsParamsV1) (lanWorldsResultV1, bool, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	if params == nil {
		worlds, err := lan.LANWorlds(ctx)
		if err != nil {
			return lanWorldsResultV1{}, false, err
		}
		l.id++
		l.worlds = worlds
		return lanWorldsPage(l.id, l.worlds, 0), true, nil
	}
	if params.Snapshot != l.id || params.Offset <= 0 || params.Offset >= len(l.worlds) {
		return lanWorldsResultV1{}, false, nil
	}
	return lanWorldsPage(l.id, l.worlds, params.Offset), true, nil
}

// lanWorldsPage fits as many worlds from offset as one control frame holds. A world too large
// for a frame on its own is sent with its display text shortened; its join address is kept.
func lanWorldsPage(snapshot uint64, worlds []bridgecontract.LANWorld, offset int) lanWorldsResultV1 {
	result := lanWorldsResultV1{SchemaVersion: bridgecontract.ControlSchemaVersion, Snapshot: snapshot, Worlds: []bridgecontract.LANWorld{}}
	for _, world := range worlds[offset:] {
		result.Worlds = append(result.Worlds, world)
		if encodedLen(result) <= frameBudget {
			continue
		}
		result.Worlds = result.Worlds[:len(result.Worlds)-1]
		if len(result.Worlds) == 0 {
			result.Worlds = append(result.Worlds, shortenedLANWorld(world))
		}
		break
	}
	if end := offset + len(result.Worlds); end < len(worlds) {
		result.Next = end
	}
	return result
}

// maxLANTextBytes bounds each display string of a world that cannot fit a frame whole.
const maxLANTextBytes = 1024

// shortenedLANWorld cuts a world's display strings to maxLANTextBytes at a rune boundary.
func shortenedLANWorld(world bridgecontract.LANWorld) bridgecontract.LANWorld {
	for _, text := range []*string{&world.LevelName, &world.ServerName, &world.GameMode, &world.Version, &world.Transport} {
		if len(*text) > maxLANTextBytes {
			cut := maxLANTextBytes
			for cut > 0 && !utf8.RuneStart((*text)[cut]) {
				cut--
			}
			*text = (*text)[:cut]
		}
	}
	return world
}

// decodeLANWorldsParams reads optional paging parameters; ok is false when they are malformed.
func decodeLANWorldsParams(raw json.RawMessage) (*lanWorldsParamsV1, bool) {
	if len(raw) == 0 {
		return nil, true
	}
	var params lanWorldsParamsV1
	if !decodeParams(raw, &params) {
		return nil, false
	}
	return &params, true
}
