package protocol

import (
	"fmt"
	"strconv"
	"strings"
)

// Field length limits, in UTF-16 units, that 1.7.10 enforces while reading.
const (
	maxUsernameChars  = 16
	maxUUIDChars      = 36
	maxServerIDChars  = 20
	maxHostnameChars  = 255
	maxLevelTypeChars = 16
)

// Handshake opens every connection and selects the state that follows.
type Handshake struct {
	handshakeServerbound
	ProtocolVersion int32
	// ServerAddress and ServerPort are what the client connected to, after any SRV lookup.
	ServerAddress string
	ServerPort    uint16
	// NextState is StateStatus or StateLogin.
	NextState State
}

func (*Handshake) ID() int32 { return 0x00 }

func (p *Handshake) Marshal(w *Writer) {
	w.VarInt(p.ProtocolVersion)
	w.String(p.ServerAddress)
	w.Uint16(p.ServerPort)
	w.VarInt(int32(p.NextState))
}

func (p *Handshake) Unmarshal(r *Reader) error {
	p.ProtocolVersion = r.VarInt()
	p.ServerAddress = r.String(maxHostnameChars)
	p.ServerPort = r.Uint16()
	p.NextState = State(r.VarInt())
	return r.Err()
}

// StatusRequest asks for the server list entry.
type StatusRequest struct {
	statusServerbound
}

func (*StatusRequest) ID() int32 { return 0x00 }

func (*StatusRequest) Marshal(*Writer) {}

func (*StatusRequest) Unmarshal(r *Reader) error { return r.Err() }

// StatusPing carries a client timestamp for the server to echo.
type StatusPing struct {
	statusServerbound
	Time int64
}

func (*StatusPing) ID() int32 { return 0x01 }

func (p *StatusPing) Marshal(w *Writer) { w.Int64(p.Time) }

func (p *StatusPing) Unmarshal(r *Reader) error {
	p.Time = r.Int64()
	return r.Err()
}

// StatusResponse carries the server list entry as raw JSON.
type StatusResponse struct {
	statusClientbound
	JSON string
}

func (*StatusResponse) ID() int32 { return 0x00 }

func (p *StatusResponse) Marshal(w *Writer) { w.String(p.JSON) }

func (p *StatusResponse) Unmarshal(r *Reader) error {
	p.JSON = r.String(MaxStringChars)
	return r.Err()
}

// StatusPong echoes a StatusPing.
type StatusPong struct {
	statusClientbound
	Time int64
}

func (*StatusPong) ID() int32 { return 0x01 }

func (p *StatusPong) Marshal(w *Writer) { w.Int64(p.Time) }

func (p *StatusPong) Unmarshal(r *Reader) error {
	p.Time = r.Int64()
	return r.Err()
}

// LoginStart names the player who wants to join.
type LoginStart struct {
	loginServerbound
	Username string
}

func (*LoginStart) ID() int32 { return 0x00 }

func (p *LoginStart) Marshal(w *Writer) { w.String(p.Username) }

func (p *LoginStart) Unmarshal(r *Reader) error {
	p.Username = r.String(maxUsernameChars)
	return r.Err()
}

// EncryptionResponse returns the shared secret and the verify token, each encrypted with the
// server's public key.
type EncryptionResponse struct {
	loginServerbound
	SharedSecret []byte
	VerifyToken  []byte
}

func (*EncryptionResponse) ID() int32 { return 0x01 }

func (p *EncryptionResponse) Marshal(w *Writer) {
	w.ByteArray16(p.SharedSecret)
	w.ByteArray16(p.VerifyToken)
}

func (p *EncryptionResponse) Unmarshal(r *Reader) error {
	p.SharedSecret = r.ByteArray16()
	p.VerifyToken = r.ByteArray16()
	return r.Err()
}

// LoginDisconnect refuses the login. Reason is a raw JSON chat component.
type LoginDisconnect struct {
	loginClientbound
	Reason string
}

func (*LoginDisconnect) ID() int32 { return 0x00 }

func (p *LoginDisconnect) Marshal(w *Writer) { w.String(p.Reason) }

func (p *LoginDisconnect) Unmarshal(r *Reader) error {
	p.Reason = r.String(MaxStringChars)
	return r.Err()
}

// EncryptionRequest starts online-mode authentication. PublicKey is the server's RSA key in
// X.509 SubjectPublicKeyInfo DER form.
type EncryptionRequest struct {
	loginClientbound
	ServerID    string
	PublicKey   []byte
	VerifyToken []byte
}

func (*EncryptionRequest) ID() int32 { return 0x01 }

func (p *EncryptionRequest) Marshal(w *Writer) {
	w.String(p.ServerID)
	w.ByteArray16(p.PublicKey)
	w.ByteArray16(p.VerifyToken)
}

func (p *EncryptionRequest) Unmarshal(r *Reader) error {
	p.ServerID = r.String(maxServerIDChars)
	p.PublicKey = r.ByteArray16()
	p.VerifyToken = r.ByteArray16()
	return r.Err()
}

// LoginSuccess ends the login state; the connection is in the play state afterwards.
type LoginSuccess struct {
	loginClientbound
	// UUID is the player's id in its textual form.
	UUID     string
	Username string
}

func (*LoginSuccess) ID() int32 { return 0x02 }

func (p *LoginSuccess) Marshal(w *Writer) {
	w.String(p.UUID)
	w.String(p.Username)
}

// Unmarshal rejects an id that is not UUID text, because 1.7.10 parses it with java.util.UUID
// while reading.
func (p *LoginSuccess) Unmarshal(r *Reader) error {
	p.UUID = r.String(maxUUIDChars)
	p.Username = r.String(maxUsernameChars)
	if err := r.Err(); err != nil {
		return err
	}
	return checkUUIDText(p.UUID)
}

// checkUUIDText reports whether s has the shape the client's UUID parser takes: five
// dash-separated hexadecimal numbers. The parser does not require the canonical digit counts.
func checkUUIDText(s string) error {
	parts := strings.Split(s, "-")
	for len(parts) > 0 && parts[len(parts)-1] == "" {
		parts = parts[:len(parts)-1]
	}
	if len(parts) != 5 {
		return fmt.Errorf("invalid UUID %q", s)
	}
	for _, part := range parts {
		if _, err := strconv.ParseInt(part, 16, 64); err != nil {
			return fmt.Errorf("invalid UUID %q", s)
		}
	}
	return nil
}
