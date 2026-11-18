package proxy

import (
	"net"
	"testing"
	"testing/synctest"

	"github.com/pion/logging"
	"github.com/pion/sctp"
)

// TestNetherNetSCTPBundlesOwedSACKWithReply pins the SCTP that NetherNet runs on to bundle a
// SACK it still owes with the DATA it sends, as vanilla does. A server measures its round trip
// from these SACKs, so a SACK held for the 200 ms delayed-ack timer reads as 200 ms of ping.
func TestNetherNetSCTPBundlesOwedSACKWithReply(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		clientConn, serverConn := net.Pipe()
		loggers := logging.NewDefaultLoggerFactory()
		accepted := make(chan *sctp.Association, 1)
		go func() {
			server, err := sctp.Server(sctp.Config{NetConn: serverConn, LoggerFactory: loggers})
			if err != nil {
				t.Error(err)
			}
			accepted <- server
		}()
		client, err := sctp.Client(sctp.Config{NetConn: clientConn, LoggerFactory: loggers})
		if err != nil {
			t.Fatal(err)
		}
		server := <-accepted
		if server == nil {
			t.FailNow()
		}
		defer func() {
			_ = client.Close()
			_ = server.Close()
		}()

		stream, err := client.OpenStream(0, sctp.PayloadTypeWebRTCBinary)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := stream.Write([]byte("ping")); err != nil {
			t.Fatal(err)
		}
		peer, err := server.AcceptStream()
		if err != nil {
			t.Fatal(err)
		}
		buf := make([]byte, 16)
		if _, err := peer.Read(buf); err != nil {
			t.Fatal(err)
		}
		// Let the server finish handling the packet, so its SACK is owed when the reply leaves.
		// Virtual time does not advance while goroutines can still run, so the delayed-ack
		// timer has not fired.
		synctest.Wait()
		if _, err := peer.Write([]byte("pong")); err != nil {
			t.Fatal(err)
		}
		if _, err := stream.Read(buf); err != nil {
			t.Fatal(err)
		}
		synctest.Wait()

		// The clock still reads the instant the ping was sent, so an acknowledged ping means
		// the SACK came with the reply rather than after the 200 ms delayed-ack timer.
		if pending := stream.BufferedAmount(); pending != 0 {
			t.Fatalf("%d ping bytes are unacknowledged; the server held its SACK instead of bundling it with the reply", pending)
		}
		if srtt := client.SRTT(); srtt != 0 {
			t.Fatalf("client SRTT = %.0f ms; the pipe adds no delay, so the SACK was held", srtt)
		}
	})
}
