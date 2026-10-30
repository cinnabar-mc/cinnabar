package main

import (
	"strings"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// Real player teardown must work with no Experience host, regardless of which peer closes first.
func TestServerStopsWithoutExperiencesWithConnectedPlayer(t *testing.T) {
	for _, clientClosesFirst := range []bool{false, true} {
		name := "server_closes_first"
		if clientClosesFirst {
			name = "client_closes_first"
		}
		t.Run(name, func(t *testing.T) {
			addr := freeAddr(t)
			stop := startServer(t, []string{"-dir", t.TempDir(), "-addr", addr})
			conn, err := (minecraft.Dialer{IdentityData: login.IdentityData{DisplayName: "ShutdownFixture"}}).DialTimeout("nethernet", "http://"+addr, 20*time.Second)
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = conn.Close() })
			if err := conn.SetDeadline(time.Now().Add(20 * time.Second)); err != nil {
				t.Fatal(err)
			}
			if err := conn.DoSpawn(); err != nil {
				t.Fatal(err)
			}
			// Spawn is sent before the accept callback installs the handler. A command reply
			// confirms the session can dispatch application work on that same world owner.
			if err := conn.WritePacket(&packet.CommandRequest{CommandLine: "/speed 2"}); err != nil {
				t.Fatal(err)
			}
			if err := conn.Flush(); err != nil {
				t.Fatal(err)
			}
			readUntil(t, conn, "accepted player's speed command reply", func(pk packet.Packet) bool {
				switch output := pk.(type) {
				case *packet.Text:
					return strings.Contains(output.Message, "Speed x2:")
				case *packet.CommandOutput:
					for _, message := range output.OutputMessages {
						if strings.Contains(message.Message, "Speed x2:") {
							return true
						}
					}
				}
				return false
			})
			if clientClosesFirst {
				_ = conn.Close()
			}
			stop()
		})
	}
}
