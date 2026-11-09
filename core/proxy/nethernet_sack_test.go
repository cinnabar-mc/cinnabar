package proxy

import (
	"net"
	"testing"
	"testing/synctest"
	"time"

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
		// The reply leaves while the server still owes a SACK for the single DATA chunk.
		if _, err := peer.Write([]byte("pong")); err != nil {
			t.Fatal(err)
		}
		if _, err := stream.Read(buf); err != nil {
			t.Fatal(err)
		}
		// Let the delayed-ack timer fire, so the client has measured its first round trip
		// whether or not the server bundled the SACK.
		time.Sleep(time.Second)
		synctest.Wait()

		// The pipe adds no delay, so only a held SACK can make the round trip nonzero.
		if srtt := client.SRTT(); srtt >= 100 {
			t.Fatalf("client SRTT = %.0f ms; the server held its SACK instead of bundling it with the reply", srtt)
		}
	})
}
