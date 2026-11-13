// Package conn connects to Minecraft Java Edition 1.7.10 servers: it resolves the address,
// performs the handshake and login, including online-mode encryption, and then exchanges play
// packets.
package conn

import (
	"context"
	"errors"
	"fmt"
	"net"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
)

// DefaultReadTimeout is how long 1.7.10 waits for the server to send anything before it
// drops the connection.
const DefaultReadTimeout = 20 * time.Second

// ErrAuthenticationRequired reports that the server asked for online-mode authentication but
// Options.JoinSession was not set: the server requires an authenticated Java account.
var ErrAuthenticationRequired = errors.New("conn: server requires an authenticated Java account")

// DisconnectError reports that the server refused the login. Reason is the raw JSON chat
// component it sent.
type DisconnectError struct {
	Reason string
}

// Error describes the refusal.
func (e *DisconnectError) Error() string { return "conn: server refused login: " + e.Reason }

// SessionError reports that Options.JoinSession failed, so the login could not be
// authenticated. Err is the error it returned.
type SessionError struct {
	Err error
}

// Error describes the failure.
func (e *SessionError) Error() string { return "conn: session join failed: " + e.Err.Error() }

// Unwrap returns the error JoinSession returned.
func (e *SessionError) Unwrap() error { return e.Err }

// Options configures Dial.
type Options struct {
	// Username is the player name sent in LoginStart.
	Username string
	// JoinSession tells the session service that the account is joining the server
	// identified by serverHash. It runs when the server requests encryption, before the
	// client answers. When nil, such a request fails the login with
	// ErrAuthenticationRequired.
	JoinSession func(ctx context.Context, serverHash string) error
	// Dialer opens the TCP connection. The default is a net.Dialer.
	Dialer func(ctx context.Context, network, address string) (net.Conn, error)
	// Resolver looks up the SRV record. The default is net.DefaultResolver.
	Resolver Resolver
	// DialTimeout bounds address resolution and the TCP connect when positive.
	DialTimeout time.Duration
	// ReadTimeout fails a read once the server has sent nothing for this long. Zero means
	// DefaultReadTimeout and a negative value disables it.
	ReadTimeout time.Duration
	// WriteTimeout bounds each Flush when positive.
	WriteTimeout time.Duration
}

// Profile is the identity the server assigned in LoginSuccess.
type Profile struct {
	// UUID is the player's id in its textual form.
	UUID     string
	Username string
}

// Conn is a logged-in connection in the play state. One goroutine may call ReadPacket while
// others call WritePacket and Flush; ReadPacket must not be called concurrently with itself.
// Close may be called from any goroutine and unblocks the others.
type Conn struct {
	transport *Transport
	address   Address
	profile   Profile
}

// Dial connects to the server at address, written as a player would type it, and logs in as
// opts.Username. It returns once the server has sent LoginSuccess. Cancelling ctx aborts the
// attempt and closes the connection. A server that refuses the login yields a
// *DisconnectError.
func Dial(ctx context.Context, address string, opts Options) (*Conn, error) {
	target, netConn, err := connect(ctx, address, opts)
	if err != nil {
		return nil, err
	}
	// Closing the stream is what interrupts a blocked login read when ctx ends.
	stop := context.AfterFunc(ctx, func() { _ = netConn.Close() })
	c := &Conn{
		transport: NewTransport(netConn, readTimeout(opts), opts.WriteTimeout),
		address:   target,
	}
	err = c.login(ctx, opts)
	if !stop() && err == nil {
		err = ctx.Err()
	}
	if err != nil {
		_ = netConn.Close()
		if ctxErr := ctx.Err(); ctxErr != nil {
			return nil, fmt.Errorf("log in to %s: %w", target, ctxErr)
		}
		return nil, fmt.Errorf("log in to %s: %w", target, err)
	}
	return c, nil
}

// readTimeout returns the idle read timeout opts asks for.
func readTimeout(opts Options) time.Duration {
	switch {
	case opts.ReadTimeout == 0:
		return DefaultReadTimeout
	case opts.ReadTimeout < 0:
		return 0
	}
	return opts.ReadTimeout
}

// connect resolves address and opens the stream to it.
func connect(ctx context.Context, address string, opts Options) (Address, net.Conn, error) {
	if opts.DialTimeout > 0 {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, opts.DialTimeout)
		defer cancel()
	}
	resolver := opts.Resolver
	if resolver == nil {
		resolver = net.DefaultResolver
	}
	target, err := ResolveAddress(ctx, resolver, address)
	if err != nil {
		return Address{}, nil, err
	}
	dial := opts.Dialer
	if dial == nil {
		dial = (&net.Dialer{}).DialContext
	}
	netConn, err := dial(ctx, "tcp", target.String())
	if err != nil {
		return Address{}, nil, fmt.Errorf("connect to %s: %w", target, err)
	}
	return target, netConn, nil
}

// login runs the handshake and login states to completion.
func (c *Conn) login(ctx context.Context, opts Options) error {
	handshake := &protocol.Handshake{
		ProtocolVersion: protocol.Version,
		ServerAddress:   c.address.Host,
		ServerPort:      c.address.Port,
		NextState:       protocol.StateLogin,
	}
	if err := c.send(handshake, &protocol.LoginStart{Username: opts.Username}); err != nil {
		return err
	}
	encrypted := false
	for {
		payload, err := c.transport.ReadFrame()
		if err != nil {
			return err
		}
		p, err := protocol.Decode(protocol.StateLogin, protocol.Clientbound, payload)
		if err != nil {
			return err
		}
		switch p := p.(type) {
		case *protocol.LoginDisconnect:
			return &DisconnectError{Reason: p.Reason}
		case *protocol.EncryptionRequest:
			if encrypted {
				return errors.New("server requested encryption twice")
			}
			if err := c.encrypt(ctx, opts, p); err != nil {
				return err
			}
			encrypted = true
		case *protocol.LoginSuccess:
			c.profile = Profile{UUID: p.UUID, Username: p.Username}
			return nil
		default:
			return fmt.Errorf("bad login packet id %d", p.ID())
		}
	}
}

// encrypt answers an EncryptionRequest: it authenticates with the session service, returns
// the shared secret and verify token under the server's key, and switches the stream to
// AES/CFB8 once that answer is sent.
func (c *Conn) encrypt(ctx context.Context, opts Options, request *protocol.EncryptionRequest) error {
	key, err := parsePublicKey(request.PublicKey)
	if err != nil {
		return err
	}
	secret, err := newSharedSecret()
	if err != nil {
		return err
	}
	if opts.JoinSession == nil {
		return ErrAuthenticationRequired
	}
	if err := opts.JoinSession(ctx, ServerHash(request.ServerID, secret, request.PublicKey)); err != nil {
		return &SessionError{Err: err}
	}
	response := &protocol.EncryptionResponse{}
	if response.SharedSecret, err = encryptForServer(key, secret); err != nil {
		return err
	}
	if response.VerifyToken, err = encryptForServer(key, request.VerifyToken); err != nil {
		return err
	}
	if err := c.send(response); err != nil {
		return err
	}
	return c.transport.EnableEncryption(secret)
}

// send queues packets of any state and flushes them.
func (c *Conn) send(packets ...protocol.Packet) error {
	for _, p := range packets {
		payload, err := protocol.Encode(p)
		if err != nil {
			return err
		}
		if err := c.transport.WriteFrame(payload); err != nil {
			return err
		}
	}
	return c.transport.Flush()
}

// ReadPacket returns the next play packet from the server. A frame with an id the protocol
// does not define is returned as a *protocol.Unknown so callers can count and skip it. A
// malformed frame, including one with bytes left after its packet, is an error; 1.7.10
// drops the connection for those, and the stream should be closed.
func (c *Conn) ReadPacket() (protocol.Packet, error) {
	payload, err := c.transport.ReadFrame()
	if err != nil {
		return nil, err
	}
	return protocol.Decode(protocol.StatePlay, protocol.Clientbound, payload)
}

// WritePacket queues a serverbound play packet. Nothing is sent before Flush. A packet whose
// frame would exceed MaxFrameSize is ErrFrameTooLarge.
func (c *Conn) WritePacket(p protocol.Packet) error {
	if p.State() != protocol.StatePlay || p.Direction() != protocol.Serverbound {
		return fmt.Errorf("conn: cannot send %s %s packet %#02x in the play state", p.State(), p.Direction(), p.ID())
	}
	payload, err := protocol.Encode(p)
	if err != nil {
		return err
	}
	return c.transport.WriteFrame(payload)
}

// Flush sends every packet queued by WritePacket.
func (c *Conn) Flush() error { return c.transport.Flush() }

// Close closes the connection. Blocked ReadPacket and Flush calls return an error.
func (c *Conn) Close() error { return c.transport.Close() }

// LocalAddr returns the local network address.
func (c *Conn) LocalAddr() net.Addr { return c.transport.LocalAddr() }

// RemoteAddr returns the server's network address.
func (c *Conn) RemoteAddr() net.Addr { return c.transport.RemoteAddr() }

// Profile returns the identity the server assigned at login.
func (c *Conn) Profile() Profile { return c.profile }

// Address returns the host and port the connection was made to and reported in the Handshake.
func (c *Conn) Address() Address { return c.address }
