// Package lanworlds browses the local network for worlds while the launcher lists them, and keeps
// the browser open while a listed NetherNet world is joined through it.
package lanworlds

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/df-mc/go-nethernet/discovery"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/sandertv/gophertunnel/minecraft"
)

// Wire transports reported in LANWorld.Transport.
const (
	TransportNetherNet = "nethernet"
	TransportRakNet    = "raknet"
)

// rakNetTargetPrefix follows the LAN target prefix for a RakNet world's host:port.
const rakNetTargetPrefix = "raknet/"

// RakNetTarget returns the LAN target joining the RakNet world at hostPort directly, without the
// address probe servers added by hand get.
func RakNetTarget(hostPort string) string {
	return bridgecontract.LANTargetPrefix + rakNetTargetPrefix + hostPort
}

// ParseTarget splits a LAN target value, after its prefix, into a NetherNet network ID or a RakNet
// host:port; ok is false when it is neither.
func ParseTarget(value string) (networkID uint64, hostPort string, ok bool) {
	if rest, found := strings.CutPrefix(value, rakNetTargetPrefix); found {
		host, port, err := net.SplitHostPort(rest)
		number, portErr := strconv.ParseUint(port, 10, 16)
		if err != nil || portErr != nil || number == 0 || host == "" {
			return 0, "", false
		}
		return 0, rest, true
	}
	id, err := strconv.ParseUint(value, 10, 64)
	return id, "", err == nil && id != 0
}

// idleTimeout closes the browser once the launcher stops listing worlds and no join holds it.
const idleTimeout = 30 * time.Second

// ErrNotFound reports a NetherNet world that no longer answers on the local network.
var ErrNotFound = errors.New("lanworlds: world is no longer on the local network")

// Browser is the part of minecraft.LANBrowser the service uses.
type Browser interface {
	Worlds() []minecraft.LANWorld
	NetherNet() *discovery.Listener
	Close() error
}

// Service lists LAN worlds on demand and resolves listed NetherNet worlds for joining.
type Service struct {
	open func() (Browser, error)
	log  *slog.Logger

	mu       sync.Mutex
	browser  Browser
	lastUse  time.Time
	leases   int
	idleStop *time.Timer
}

// New returns a Service that opens a gophertunnel LAN browser when first asked.
func New(log *slog.Logger) *Service {
	if log == nil {
		log = slog.Default()
	}
	return newService(func() (Browser, error) {
		return minecraft.LANBrowserConfig{Log: log}.Browse()
	}, log)
}

// newService builds a Service around an injectable browser.
func newService(open func() (Browser, error), log *slog.Logger) *Service {
	return &Service{open: open, log: log}
}

// Worlds returns the worlds found so far, in the browser's order, starting the browser if needed.
func (s *Service) Worlds() ([]bridgecontract.LANWorld, error) {
	browser, err := s.use()
	if err != nil {
		return nil, err
	}
	found := browser.Worlds()
	worlds := make([]bridgecontract.LANWorld, 0, len(found))
	for _, world := range found {
		worlds = append(worlds, wireWorld(world))
	}
	return worlds, nil
}

// Find waits until the NetherNet world with networkID is listed and returns it with the signaling
// that reaches it. The browser stays open until release is called. It fails once ctx ends without
// the world answering.
func (s *Service) Find(ctx context.Context, networkID uint64) (world minecraft.LANWorld, signaling nethernet.Signaling, release func(), err error) {
	browser, err := s.use()
	if err != nil {
		return minecraft.LANWorld{}, nil, nil, err
	}
	s.mu.Lock()
	s.leases++
	s.mu.Unlock()
	var once sync.Once
	release = func() {
		once.Do(func() {
			s.mu.Lock()
			s.leases--
			s.lastUse = time.Now()
			s.scheduleIdleLocked()
			s.mu.Unlock()
		})
	}
	ticker := time.NewTicker(100 * time.Millisecond)
	defer ticker.Stop()
	for {
		for _, world := range browser.Worlds() {
			if world.NetherNet && world.NetworkID == networkID {
				return world, browser.NetherNet(), release, nil
			}
		}
		select {
		case <-ctx.Done():
			release()
			return minecraft.LANWorld{}, nil, nil, fmt.Errorf("%w: %w", ErrNotFound, context.Cause(ctx))
		case <-ticker.C:
		}
	}
}

// Close stops browsing.
func (s *Service) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.idleStop != nil {
		s.idleStop.Stop()
	}
	return s.closeLocked()
}

// use returns the open browser, opening it when needed, and records the use.
func (s *Service) use() (Browser, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.browser == nil {
		browser, err := s.open()
		if err != nil {
			return nil, fmt.Errorf("lanworlds: start browsing: %w", err)
		}
		s.browser = browser
	}
	s.lastUse = time.Now()
	s.scheduleIdleLocked()
	return s.browser, nil
}

// scheduleIdleLocked arms the idle check; s.mu must be held.
func (s *Service) scheduleIdleLocked() {
	if s.idleStop != nil {
		s.idleStop.Stop()
	}
	s.idleStop = time.AfterFunc(idleTimeout, s.closeIfIdle)
}

// closeIfIdle closes the browser when nothing used it for idleTimeout and no join holds it.
func (s *Service) closeIfIdle() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.leases > 0 || time.Now().Sub(s.lastUse) < idleTimeout {
		return
	}
	if err := s.closeLocked(); err != nil {
		s.log.Debug("close LAN browser", "error", err)
	}
}

// closeLocked closes the browser if open; s.mu must be held.
func (s *Service) closeLocked() error {
	if s.browser == nil {
		return nil
	}
	err := s.browser.Close()
	s.browser = nil
	return err
}

// wireWorld maps a found world to its control-plane row; Address joins it from the menu.
func wireWorld(world minecraft.LANWorld) bridgecontract.LANWorld {
	wire := bridgecontract.LANWorld{
		LevelName:  world.LevelName,
		ServerName: world.ServerName,
		GameMode:   world.GameMode,
		Hardcore:   world.Hardcore,
		Players:    uint32(max(world.PlayerCount, 0)),
		MaxPlayers: uint32(max(world.MaxPlayerCount, 0)),
		Version:    world.Version,
	}
	if world.NetherNet {
		wire.Transport = TransportNetherNet
		wire.Address = bridgecontract.LANTargetPrefix + strconv.FormatUint(world.NetworkID, 10)
		return wire
	}
	wire.Transport = TransportRakNet
	if world.Address != nil {
		wire.Address = RakNetTarget(net.JoinHostPort(world.Address.IP.String(), strconv.Itoa(world.Address.Port)))
	}
	return wire
}
