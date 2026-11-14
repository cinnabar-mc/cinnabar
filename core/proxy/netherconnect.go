package proxy

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"net"
	"strings"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// netherConnectTransfer recognizes NetherConnect's loopback transfer server by its RakNet pong,
// then reads the transfer without treating its synthetic legacy StartGame as a real world.
// Its handshake is documented by https://github.com/GameParrot/netherconnect/blob/main/transfer_server.go.
func netherConnectTransfer(ctx context.Context, address string, proto minecraft.Protocol) (*minecraft.TransferError, bool, error) {
	if proto == nil {
		proto = minecraft.DefaultProtocol
	}
	host, _, err := net.SplitHostPort(address)
	if err != nil {
		return nil, false, nil
	}
	ip := net.ParseIP(host)
	if !(ip != nil && ip.IsLoopback()) && !strings.EqualFold(host, "localhost") {
		return nil, false, nil
	}
	probe, cancel := context.WithTimeout(ctx, 500*time.Millisecond)
	pong, err := remoteRakNet().PingContext(probe, address)
	cancel()
	if err != nil || !strings.HasPrefix(string(pong), "MCPE;NetherConnect;") {
		return nil, false, nil
	}
	conn, err := dialTransport(ctx, remoteRakNet(), address)
	if err != nil {
		return nil, true, err
	}
	defer conn.Close()
	stop := context.AfterFunc(ctx, func() { _ = conn.Close() })
	defer stop()
	encoder, decoder := packet.NewEncoder(conn), packet.NewDecoder(conn)
	request := new(bytes.Buffer)
	if err := (&packet.Header{PacketID: packet.IDRequestNetworkSettings}).Write(request); err != nil {
		return nil, true, err
	}
	(&packet.RequestNetworkSettings{ClientProtocol: proto.ID()}).Marshal(proto.NewWriter(request, 0))
	if err := encoder.Encode([][]byte{request.Bytes()}); err != nil {
		return nil, true, err
	}
	settings, status := false, false
	for {
		batch, err := decoder.Decode()
		if err != nil {
			return nil, true, fmt.Errorf("netherconnect transfer: %w", err)
		}
		for _, data := range batch {
			buf := bytes.NewBuffer(data)
			var header packet.Header
			if err := header.Read(buf); err != nil {
				return nil, true, err
			}
			switch header.PacketID {
			case packet.IDNetworkSettings:
				value, ok := decodeSessionPacket(minecraft.DefaultProtocol.Packets(false), data, 0, true).(*packet.NetworkSettings)
				if !ok || value.CompressionAlgorithm != packet.FlateCompression.EncodeCompression() || settings {
					return nil, true, errors.New("netherconnect transfer: invalid network settings")
				}
				settings = true
			case packet.IDPlayStatus:
				value, ok := decodeSessionPacket(minecraft.DefaultProtocol.Packets(false), data, 0, true).(*packet.PlayStatus)
				if !settings || status || !ok || value.Status != packet.PlayStatusLoginSuccess {
					return nil, true, errors.New("netherconnect transfer: invalid login status")
				}
				status = true
				decoder.EnableCompression(packet.FlateCompression, maxUpstreamDecompressedBatch)
			case packet.IDTransfer:
				if !status {
					return nil, true, errors.New("netherconnect transfer: transfer before login status")
				}
				transfer, err := netherConnectTransferPacket(buf.Bytes(), proto)
				return transfer, true, err
			}
		}
	}
}

// netherConnectTransferPacket accepts the legacy address, port and reload fields exactly.
func netherConnectTransferPacket(data []byte, proto minecraft.Protocol) (result *minecraft.TransferError, err error) {
	defer func() {
		if recover() != nil {
			result, err = nil, errors.New("netherconnect transfer: malformed packet")
		}
	}()
	buf := bytes.NewBuffer(data)
	reader := proto.NewReader(buf, 0, true)
	transfer := &minecraft.TransferError{}
	reader.String(&transfer.Address)
	reader.Uint16(&transfer.Port)
	reader.Bool(&transfer.ReloadWorld)
	if buf.Len() != 0 || transfer.Port != 0 || !isRawNetherNetAddress(transfer.Address) {
		return nil, errors.New("netherconnect transfer: invalid identity")
	}
	return transfer, nil
}
