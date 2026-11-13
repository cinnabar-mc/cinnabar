package translate

import (
	"bufio"
	"compress/gzip"
	"encoding/json"
	"math"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"testing"

	"github.com/go-gl/mathgl/mgl32"
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
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

// The client completes its single-precision position with two remainders per axis; the
// fixture is written by the client's own packet builder.
func TestExactPositionReadsTheClientsRemainders(t *testing.T) {
	file, err := os.Open("../../../crates/protocol/fixtures/java_positions.tsv")
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	rows := 0
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		line := scanner.Text()
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		fields := strings.Split(line, "\t")
		if len(fields) != 12 {
			t.Fatalf("row %q has %d fields", line, len(fields))
		}
		single := func(index int) float32 {
			bits, err := strconv.ParseUint(fields[index], 16, 32)
			if err != nil {
				t.Fatal(err)
			}
			return math.Float32frombits(uint32(bits))
		}
		var want [3]uint64
		for axis := range want {
			if want[axis], err = strconv.ParseUint(fields[axis], 16, 64); err != nil {
				t.Fatal(err)
			}
		}
		pk := &packet.PlayerAuthInput{
			Position:          mgl32.Vec3{single(3), single(4), single(5)},
			Delta:             mgl32.Vec3{single(6), single(7), single(8)},
			CameraOrientation: mgl32.Vec3{single(9), single(10), single(11)},
		}
		x, feet, z := exactPosition(pk)
		if got := [3]uint64{math.Float64bits(x), math.Float64bits(feet), math.Float64bits(z)}; got != want {
			t.Fatalf("row %q restored as %v, %v, %v", line, x, feet, z)
		}
		rows++
	}
	if err := scanner.Err(); err != nil {
		t.Fatal(err)
	}
	if rows < 40 {
		t.Fatalf("the fixture lists only %d positions", rows)
	}
}

// A Bedrock client's delta and camera orientation are no remainders: the position is read
// as the packet gives it.
func TestExactPositionIgnoresAnOrdinaryDeltaAndOrientation(t *testing.T) {
	pk := &packet.PlayerAuthInput{
		Position:          mgl32.Vec3{8.5, 5.62001, -300.25},
		Delta:             mgl32.Vec3{0.1, -0.0784, 0.2},
		CameraOrientation: mgl32.Vec3{0.6, 0, 0.8},
	}
	x, feet, z := exactPosition(pk)
	if x != 8.5 || feet != float64(float32(5.62001))-networkOffset || z != -300.25 {
		t.Fatalf("position read as %v, %v, %v", x, feet, z)
	}
}
