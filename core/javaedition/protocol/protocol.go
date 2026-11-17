// Package protocol implements the wire format of Minecraft Java Edition 1.7.10 (protocol
// version 5): the primitive encodings and every packet of the handshake, status, login and play
// states. Packets keep their wire representation; interpreting it is left to callers.
package protocol

import (
	"errors"
	"fmt"
)

// Version is the protocol number a 1.7.10 client sends in its Handshake.
const Version int32 = 5

// State is a connection phase. Each state has its own packet ids. The values are the numbers
// 1.7.10 uses for the Handshake's requested state.
type State int32

const (
	// StateHandshake is the state every connection starts in.
	StateHandshake State = -1
	// StatePlay carries the game itself.
	StatePlay State = 0
	// StateStatus serves the server list ping.
	StateStatus State = 1
	// StateLogin covers authentication and encryption setup.
	StateLogin State = 2
)

// String names the state for errors and logs.
func (s State) String() string {
	switch s {
	case StateHandshake:
		return "handshake"
	case StatePlay:
		return "play"
	case StateStatus:
		return "status"
	case StateLogin:
		return "login"
	}
	return fmt.Sprintf("state(%d)", int32(s))
}

// Direction says which peer receives a packet.
type Direction uint8

const (
	// Clientbound packets travel from the server to the client.
	Clientbound Direction = iota
	// Serverbound packets travel from the client to the server.
	Serverbound
)

// String names the direction for errors and logs.
func (d Direction) String() string {
	if d == Serverbound {
		return "serverbound"
	}
	return "clientbound"
}

// Packet is one protocol message. ID, State and Direction identify it on the wire. Marshal
// appends the body, without the id, to w and reports failures through w.Err. Unmarshal reads
// the body from r and returns the first error, leaving trailing bytes for the caller to judge.
type Packet interface {
	ID() int32
	State() State
	Direction() Direction
	Marshal(w *Writer)
	Unmarshal(r *Reader) error
}

// Unknown holds a frame whose packet id has no registered packet. Data is the body after the
// id.
type Unknown struct {
	PacketID        int32
	PacketState     State
	PacketDirection Direction
	Data            []byte
}

// ID returns the unregistered packet id.
func (p *Unknown) ID() int32 { return p.PacketID }

// State returns the state the frame arrived in.
func (p *Unknown) State() State { return p.PacketState }

// Direction returns the direction the frame travelled.
func (p *Unknown) Direction() Direction { return p.PacketDirection }

// Marshal writes the body unchanged.
func (p *Unknown) Marshal(w *Writer) { w.Raw(p.Data) }

// Unmarshal takes all remaining bytes as the body.
func (p *Unknown) Unmarshal(r *Reader) error {
	p.Data = r.Bytes(r.Remaining())
	return r.Err()
}

// ErrTrailingData reports bytes left in a frame after its packet was read. 1.7.10 drops the
// connection for such a frame.
var ErrTrailingData = errors.New("protocol: packet is larger than expected")

// Encode returns the frame payload for p: its id as a VarInt followed by its body.
func Encode(p Packet) ([]byte, error) {
	w := NewWriter()
	w.VarInt(p.ID())
	p.Marshal(w)
	if err := w.Err(); err != nil {
		return nil, fmt.Errorf("encode %s %s packet %#02x: %w", p.State(), p.Direction(), p.ID(), err)
	}
	return w.Bytes(), nil
}

// Decode reads one frame payload received in state and travelling in dir. An unregistered id
// yields an *Unknown rather than an error. A body that ends early, is malformed or leaves
// bytes unread is an error.
func Decode(state State, dir Direction, payload []byte) (Packet, error) {
	r := NewReader(payload)
	id := r.VarInt()
	if err := r.Err(); err != nil {
		return nil, fmt.Errorf("decode %s %s packet id: %w", state, dir, err)
	}
	p, ok := New(state, dir, id)
	if !ok {
		return &Unknown{PacketID: id, PacketState: state, PacketDirection: dir, Data: r.Bytes(r.Remaining())}, nil
	}
	if err := p.Unmarshal(r); err != nil {
		return nil, fmt.Errorf("decode %s %s packet %#02x: %w", state, dir, id, err)
	}
	if n := r.Remaining(); n > 0 {
		return nil, fmt.Errorf("decode %s %s packet %#02x: %w: %d bytes extra", state, dir, id, ErrTrailingData, n)
	}
	return p, nil
}

// The embedded markers below give each packet its State and Direction.

type handshakeServerbound struct{}

func (handshakeServerbound) State() State         { return StateHandshake }
func (handshakeServerbound) Direction() Direction { return Serverbound }

type statusClientbound struct{}

func (statusClientbound) State() State         { return StateStatus }
func (statusClientbound) Direction() Direction { return Clientbound }

type statusServerbound struct{}

func (statusServerbound) State() State         { return StateStatus }
func (statusServerbound) Direction() Direction { return Serverbound }

type loginClientbound struct{}

func (loginClientbound) State() State         { return StateLogin }
func (loginClientbound) Direction() Direction { return Clientbound }

type loginServerbound struct{}

func (loginServerbound) State() State         { return StateLogin }
func (loginServerbound) Direction() Direction { return Serverbound }

type playClientbound struct{}

func (playClientbound) State() State         { return StatePlay }
func (playClientbound) Direction() Direction { return Clientbound }

type playServerbound struct{}

func (playServerbound) State() State         { return StatePlay }
func (playServerbound) Direction() Direction { return Serverbound }
