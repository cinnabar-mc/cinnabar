package proxy

import (
	"bytes"
	"context"
	"encoding/hex"
	"fmt"
	"io"
	"log/slog"
	"strings"
	"testing"
	"time"

	"github.com/sandertv/go-raknet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// TestNetherConnectTransferHandshake models the early server packet order in
// https://github.com/GameParrot/netherconnect/blob/main/transfer_server.go. The fixture payloads
// come from legacy-version-no-mapping commit be0f198f71b3, with the current protocol ID.
func TestNetherConnectTransferHandshake(t *testing.T) {
	for _, address := range []string{"127.0.0.1:0", "[::1]:0"} {
		t.Run(address, func(t *testing.T) { testNetherConnectTransferHandshake(t, address) })
	}
}

// testNetherConnectTransferHandshake verifies the announced loopback endpoint's transfer.
func testNetherConnectTransferHandshake(t *testing.T, listenAddress string) {
	const identity = "1234567890123456789"
	if protocol.CurrentProtocol != 2225 {
		t.Fatal("regenerate NetherConnect's handshake fixture for the current protocol")
	}
	legacy := map[uint32]string{
		packet.IDResourcePacksInfo: "0000000000000000000000000000000000000000000000",
		packet.IDResourcePackStack: "000000000000000000",
		packet.IDStartGame:         "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000a0000000000000000000000000000000000000000000000000000000000000000000000",
		packet.IDTransfer:          "1331323334353637383930313233343536373839000000",
	}
	listener, err := raknet.Listen(listenAddress)
	if err != nil {
		if strings.HasPrefix(listenAddress, "[::1]") {
			t.Skipf("missing fixture: IPv6 loopback listener: %v", err)
		}
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	listener.PongData([]byte("MCPE;NetherConnect;100;1.0;0;1;1;NetherConnect;Survival;"))
	serverDone := make(chan error, 1)
	go func() {
		conn, err := listener.Accept()
		if err != nil {
			serverDone <- err
			return
		}
		defer conn.Close()
		decoder, encoder := packet.NewDecoder(conn), packet.NewEncoder(conn)
		if _, err = decoder.Decode(); err != nil {
			serverDone <- err
			return
		}
		write := func(pk packet.Packet) error {
			buf := new(bytes.Buffer)
			if err := (&packet.Header{PacketID: pk.ID()}).Write(buf); err != nil {
				return err
			}
			if payload, ok := legacy[pk.ID()]; ok {
				decoded, err := hex.DecodeString(payload)
				if err != nil {
					return err
				}
				buf.Write(decoded)
			} else {
				pk.Marshal(minecraft.DefaultProtocol.NewWriter(buf, 0))
			}
			return encoder.Encode([][]byte{buf.Bytes()})
		}
		for _, pk := range []packet.Packet{
			&packet.NetworkSettings{CompressionAlgorithm: packet.FlateCompression.EncodeCompression()},
			&packet.PlayStatus{Status: packet.PlayStatusLoginSuccess},
		} {
			if err = write(pk); err != nil {
				serverDone <- err
				return
			}
		}
		encoder.EnableCompression(packet.FlateCompression, 1)
		for _, pk := range []packet.Packet{
			&packet.ResourcePacksInfo{}, &packet.ResourcePackStack{},
			&packet.StartGame{}, &packet.Transfer{Address: identity},
		} {
			if err = write(pk); err != nil {
				serverDone <- err
				return
			}
		}
		// Drain until the client closes, so its startup responses do not block.
		_, _ = io.Copy(io.Discard, conn)
		serverDone <- nil
	}()
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	initialAddress := listener.Addr().String()
	expectedTarget := NetherNetTargetPrefix + NetherNetSignalingJSONRPC + "/" + identity
	want := newFakeUpstream(nil)
	var addresses []string
	got, err := dialFollowingTransfers(ctx, initialAddress, slog.New(slog.DiscardHandler), func(ctx context.Context, address string) (upstreamSession, error) {
		addresses = append(addresses, address)
		if address == initialAddress {
			transfer, matched, err := netherConnectTransfer(ctx, address, minecraft.DefaultProtocol)
			if err != nil {
				return nil, err
			}
			if !matched || transfer == nil {
				return nil, fmt.Errorf("NetherConnect transfer missing: matched = %t", matched)
			}
			return nil, transfer
		}
		if address != expectedTarget {
			return nil, fmt.Errorf("unexpected transfer target %q", address)
		}
		return want, nil
	})
	if err != nil || got != want || len(addresses) != 2 {
		t.Fatalf("upstream = %v, addresses = %v, error = %v", got, addresses, err)
	}
	select {
	case err := <-serverDone:
		if err != nil {
			t.Fatal(fmt.Errorf("scripted transfer server: %w", err))
		}
	case <-ctx.Done():
		t.Fatal("scripted transfer server did not finish")
	}
}

func TestNetherConnectTransferRejectsMalformedIdentity(t *testing.T) {
	for _, payload := range [][]byte{
		nil,
		{1, 'x', 0, 0, 0},
		{1, '1', 1, 0, 0},
		{1, '1', 0, 0, 0, 0},
	} {
		if _, err := netherConnectTransferPacket(payload, minecraft.DefaultProtocol); err == nil {
			t.Fatalf("accepted invalid transfer %x", payload)
		}
	}
}

func TestNetherConnectProbeIgnoresOtherServers(t *testing.T) {
	listener, err := raknet.Listen("127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	listener.PongData([]byte("MCPE;Other Server;100;1.0;0;1;1;Other;Survival;"))
	transfer, matched, err := netherConnectTransfer(t.Context(), listener.Addr().String(), minecraft.DefaultProtocol)
	if err != nil || matched || transfer != nil {
		t.Fatalf("other server = %+v, matched = %t, error = %v", transfer, matched, err)
	}
}
