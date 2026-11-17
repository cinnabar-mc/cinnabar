package translate

import (
	"bufio"
	"compress/gzip"
	"encoding/json"
	"math"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
)

// traceTick is one tick of a recorded 1.7.10 client trace, reduced to what decides its
// movement packets. Floating-point values are raw bit patterns.
type traceTick struct {
	Events []map[string]json.RawMessage `json:"ev"`
	In     *struct {
		Yaw   int32 `json:"yaw"`
		Pitch int32 `json:"pitch"`
	} `json:"in"`
	Out *struct {
		X      int64         `json:"x"`
		MinY   int64         `json:"miny"`
		Z      int64         `json:"z"`
		Ground bool          `json:"g"`
		Sprint bool          `json:"sprint"`
		Sneak  bool          `json:"sneak"`
		Sink   int32         `json:"ysize"`
		Sent   []tracePacket `json:"pk"`
	} `json:"out"`
}

// tracePacket is a packet the recorded client sent.
type tracePacket struct {
	Kind   string `json:"k"`
	Action int8   `json:"a"`
	X      int64  `json:"x"`
	Y      int64  `json:"y"`
	Stance int64  `json:"stance"`
	Z      int64  `json:"z"`
	Yaw    int32  `json:"yaw"`
	Pitch  int32  `json:"pitch"`
	Ground bool   `json:"g"`
}

func double(bits int64) float64 { return math.Float64frombits(uint64(bits)) }
func single(bits int32) float32 { return math.Float32frombits(uint32(bits)) }

// javaPacket converts a recorded packet; packets the movement report does not produce
// return nil.
func (p tracePacket) javaPacket(entityID int32) java.Packet {
	switch p.Kind {
	case "action":
		return &java.ClientEntityAction{EntityID: entityID, Action: p.Action}
	case "ground":
		return &java.ClientPlayer{OnGround: p.Ground}
	case "pos":
		return &java.ClientPlayerPosition{X: double(p.X), Y: double(p.Y), Stance: double(p.Stance), Z: double(p.Z), OnGround: p.Ground}
	case "look":
		return &java.ClientPlayerLook{Yaw: single(p.Yaw), Pitch: single(p.Pitch), OnGround: p.Ground}
	case "poslook":
		return &java.ClientPlayerPositionLook{X: double(p.X), Y: double(p.Y), Stance: double(p.Stance), Z: double(p.Z), Yaw: single(p.Yaw), Pitch: single(p.Pitch), OnGround: p.Ground}
	}
	return nil
}

// Given the positions a recorded 1.7.10 client reached, the movement report chooses the
// packets that client sent. The camera sink of sneaking and stepping, which shifts the
// stance, is not known to the report, so ticks with one are compared without it.
func TestMotionReportMatchesRecordedClientTraces(t *testing.T) {
	traces, _ := filepath.Glob("../../../crates/java-sim/fixtures/traces/*.jsonl.gz")
	if len(traces) == 0 {
		t.Skip("missing fixture: crates/java-sim/fixtures/traces/*.jsonl.gz")
	}
	const entityID = 7
	for _, path := range traces {
		file, err := os.Open(path)
		if err != nil {
			t.Fatal(err)
		}
		reader, err := gzip.NewReader(file)
		if err != nil {
			t.Fatal(err)
		}
		scanner := bufio.NewScanner(reader)
		scanner.Buffer(nil, 1<<26)
		scanner.Scan() // the header
		var report motionReport
		ticks := 0
		for scanner.Scan() {
			var tick traceTick
			if err := json.Unmarshal(scanner.Bytes(), &tick); err != nil {
				t.Fatalf("%s: %v", path, err)
			}
			if tick.Out == nil || tick.In == nil {
				continue
			}
			var want []java.Packet
			for _, sent := range tick.Out.Sent {
				if pk := sent.javaPacket(entityID); pk != nil {
					want = append(want, pk)
				}
			}
			out := tick.Out
			got := report.next(entityID, double(out.X), double(out.MinY), double(out.Z), single(tick.In.Yaw), single(tick.In.Pitch), out.Ground, out.Sprint, out.Sneak)
			if out.Sink != 0 {
				for _, list := range [][]java.Packet{got, want} {
					for _, pk := range list {
						switch pk := pk.(type) {
						case *java.ClientPlayerPosition:
							pk.Stance = 0
						case *java.ClientPlayerPositionLook:
							pk.Stance = 0
						}
					}
				}
			}
			if !reflect.DeepEqual(got, want) {
				t.Fatalf("%s tick %d: sent %+v, the client sent %+v", filepath.Base(path), ticks, got, want)
			}
			ticks++
		}
		if err := scanner.Err(); err != nil {
			t.Fatalf("%s: %v", path, err)
		}
		_ = file.Close()
		if ticks == 0 {
			t.Fatalf("%s holds no ticks", path)
		}
	}
}
