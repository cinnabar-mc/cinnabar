// Package javatest provides a scriptable stand-in for a Minecraft Java Edition 1.7.10 server.
// It performs the server side of the handshake and login, in offline or online mode, and then
// hands each connection to the test, which reads and writes packets itself.
package javatest

import (
	"bytes"
	"context"
	"crypto/md5"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"errors"
	"fmt"
	"net"
	"sync"

	"github.com/google/uuid"

	"github.com/hashimthearab/rust-mcbe/core/javaedition/conn"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
)

const (
	// keyBits is the size of the RSA key a 1.7.10 server generates.
	keyBits = 1024
	// verifyTokenSize is the size of the token a 1.7.10 server sends with its key.
	verifyTokenSize = 4
	// pendingClients is how many logged-in connections may wait for Accept.
	pendingClients = 16
)

// Config selects how a Server treats logins.
type Config struct {
	// OnlineMode makes the server request encryption before it accepts a login.
	OnlineMode bool
	// ServerID is sent in the EncryptionRequest. 1.7.10 servers send an empty id.
	ServerID string
	// RefuseLogin, when not empty, is sent as a LoginDisconnect reason in answer to
	// LoginStart instead of continuing the login.
	RefuseLogin string
}

// Server accepts connections on a loopback port and logs them in.
type Server struct {
	config    Config
	listener  net.Listener
	key       *rsa.PrivateKey
	publicKey []byte

	accepted chan accepted
	done     chan struct{}
	close    sync.Once
	wg       sync.WaitGroup

	mu    sync.Mutex
	conns map[net.Conn]struct{}
}

// accepted is the outcome of one connection's login.
type accepted struct {
	client *Client
	err    error
}

// Start listens on 127.0.0.1 at a free port and serves logins until Close.
func Start(config Config) (*Server, error) {
	s := &Server{
		config:   config,
		accepted: make(chan accepted, pendingClients),
		done:     make(chan struct{}),
		conns:    map[net.Conn]struct{}{},
	}
	if config.OnlineMode {
		key, err := rsa.GenerateKey(rand.Reader, keyBits)
		if err != nil {
			return nil, fmt.Errorf("javatest: generate server key: %w", err)
		}
		der, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
		if err != nil {
			return nil, fmt.Errorf("javatest: encode server key: %w", err)
		}
		s.key, s.publicKey = key, der
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return nil, fmt.Errorf("javatest: listen: %w", err)
	}
	s.listener = listener
	s.wg.Add(1)
	go s.serve()
	return s, nil
}

// Addr returns the server's address in host:port form, ready to pass to conn.Dial.
func (s *Server) Addr() string { return s.listener.Addr().String() }

// PublicKey returns the encoded RSA key an online-mode server sends, nil in offline mode.
func (s *Server) PublicKey() []byte { return s.publicKey }

// Accept waits for the next connection to finish logging in. It returns the login's error
// when the client broke the protocol, and ctx's error when ctx ends first. Connections
// refused through Config.RefuseLogin are not reported.
func (s *Server) Accept(ctx context.Context) (*Client, error) {
	select {
	case result := <-s.accepted:
		return result.client, result.err
	case <-s.done:
		return nil, net.ErrClosed
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

// Close stops the listener, closes every connection and waits for the server's goroutines.
func (s *Server) Close() {
	s.close.Do(func() {
		close(s.done)
		_ = s.listener.Close()
		s.mu.Lock()
		for c := range s.conns {
			_ = c.Close()
		}
		s.mu.Unlock()
	})
	s.wg.Wait()
}

// serve accepts connections until the listener closes.
func (s *Server) serve() {
	defer s.wg.Done()
	for {
		netConn, err := s.listener.Accept()
		if err != nil {
			return
		}
		if !s.track(netConn) {
			_ = netConn.Close()
			return
		}
		s.wg.Add(1)
		go func() {
			defer s.wg.Done()
			client, err := s.login(netConn)
			if err != nil {
				_ = netConn.Close()
			}
			if client == nil && err == nil {
				return
			}
			select {
			case s.accepted <- accepted{client: client, err: err}:
			case <-s.done:
			}
		}()
	}
}

// track remembers netConn so Close can end it, and reports false once the server is closed.
func (s *Server) track(netConn net.Conn) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	select {
	case <-s.done:
		return false
	default:
	}
	s.conns[netConn] = struct{}{}
	return true
}

// login runs the handshake and login states for one connection. It returns neither a client
// nor an error when the login was refused as configured.
func (s *Server) login(netConn net.Conn) (*Client, error) {
	client := &Client{stream: netConn, transport: conn.NewTransport(netConn, 0, 0)}
	p, err := client.read(protocol.StateHandshake)
	if err != nil {
		return nil, err
	}
	handshake, ok := p.(*protocol.Handshake)
	if !ok {
		return nil, fmt.Errorf("javatest: first packet is %T, want a Handshake", p)
	}
	client.Handshake = *handshake
	if handshake.NextState != protocol.StateLogin {
		return nil, fmt.Errorf("javatest: handshake asks for the %s state; only login is served", handshake.NextState)
	}
	if p, err = client.read(protocol.StateLogin); err != nil {
		return nil, err
	}
	start, ok := p.(*protocol.LoginStart)
	if !ok {
		return nil, fmt.Errorf("javatest: got %T, want LoginStart", p)
	}
	client.Username = start.Username
	if s.config.RefuseLogin != "" {
		err := client.Write(&protocol.LoginDisconnect{Reason: s.config.RefuseLogin})
		_ = netConn.Close()
		return nil, err
	}
	if s.config.OnlineMode {
		if err := s.encrypt(client); err != nil {
			return nil, err
		}
	}
	client.UUID = OfflineUUID(start.Username)
	if err := client.Write(&protocol.LoginSuccess{UUID: client.UUID, Username: start.Username}); err != nil {
		return nil, err
	}
	return client, nil
}

// encrypt performs the online-mode key exchange and switches the connection to AES/CFB8.
func (s *Server) encrypt(client *Client) error {
	token := make([]byte, verifyTokenSize)
	if _, err := rand.Read(token); err != nil {
		return fmt.Errorf("javatest: generate verify token: %w", err)
	}
	request := &protocol.EncryptionRequest{ServerID: s.config.ServerID, PublicKey: s.publicKey, VerifyToken: token}
	if err := client.Write(request); err != nil {
		return err
	}
	p, err := client.read(protocol.StateLogin)
	if err != nil {
		return err
	}
	response, ok := p.(*protocol.EncryptionResponse)
	if !ok {
		return fmt.Errorf("javatest: got %T, want EncryptionResponse", p)
	}
	echoed, err := rsa.DecryptPKCS1v15(nil, s.key, response.VerifyToken)
	if err != nil {
		return fmt.Errorf("javatest: decrypt verify token: %w", err)
	}
	if !bytes.Equal(echoed, token) {
		return errors.New("javatest: client returned the wrong verify token")
	}
	secret, err := rsa.DecryptPKCS1v15(nil, s.key, response.SharedSecret)
	if err != nil {
		return fmt.Errorf("javatest: decrypt shared secret: %w", err)
	}
	client.SharedSecret = secret
	client.ServerHash = conn.ServerHash(s.config.ServerID, secret, s.publicKey)
	return client.transport.EnableEncryption(secret)
}

// OfflineUUID returns the id a 1.7.10 server in offline mode gives a player: a version 3 UUID
// over "OfflinePlayer:" and the name.
func OfflineUUID(username string) string {
	sum := md5.Sum([]byte("OfflinePlayer:" + username))
	sum[6] = sum[6]&0x0f | 0x30
	sum[8] = sum[8]&0x3f | 0x80
	return uuid.UUID(sum).String()
}

// Client is the server's end of one logged-in connection. One goroutine may call Read while
// another calls the write methods.
type Client struct {
	// Handshake is the packet the client opened the connection with.
	Handshake protocol.Handshake
	// Username is the name the client sent in LoginStart, and UUID the id it was given.
	Username string
	UUID     string
	// ServerHash is the hash an online-mode server would check with the session service, and
	// SharedSecret the key the client chose. Both are empty in offline mode.
	ServerHash   string
	SharedSecret []byte

	stream    net.Conn
	transport *conn.Transport
}

// read returns the next serverbound packet of the given state.
func (c *Client) read(state protocol.State) (protocol.Packet, error) {
	payload, err := c.transport.ReadFrame()
	if err != nil {
		return nil, err
	}
	return protocol.Decode(state, protocol.Serverbound, payload)
}

// Read returns the next play packet the client sent. Unregistered ids arrive as
// *protocol.Unknown.
func (c *Client) Read() (protocol.Packet, error) { return c.read(protocol.StatePlay) }

// Write sends p to the client at once.
func (c *Client) Write(p protocol.Packet) error {
	payload, err := protocol.Encode(p)
	if err != nil {
		return err
	}
	return c.WriteFrame(payload)
}

// WriteFrame sends one frame holding payload, a packet id followed by a body, without
// checking it. Tests use it to send what no packet type can express.
func (c *Client) WriteFrame(payload []byte) error {
	if err := c.transport.WriteFrame(payload); err != nil {
		return err
	}
	return c.transport.Flush()
}

// WriteBytes puts raw bytes on the stream, bypassing framing, so tests can send a broken
// frame. It works only on an unencrypted connection.
func (c *Client) WriteBytes(raw []byte) error {
	if c.SharedSecret != nil {
		return errors.New("javatest: cannot write raw bytes to an encrypted connection")
	}
	_, err := c.stream.Write(raw)
	return err
}

// Close closes the connection.
func (c *Client) Close() error { return c.transport.Close() }
