package proxy

import (
	"context"
	"slices"
	"strings"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/pion/webrtc/v4"
	"github.com/sandertv/gophertunnel/minecraft"
)

// stunGatherTimeout bounds how long gathering waits for one STUN server's answer. pion sends a
// single binding request without retransmitting it, so a lost answer held the offer for pion's
// default 5 s; an answer normally arrives within a round trip, well under this bound.
const stunGatherTimeout = 1500 * time.Millisecond

// gatheringAPI is the WebRTC API every outbound NetherNet join gathers candidates with: pion's
// defaults with stunGatherTimeout. It is safe to share between connections.
var gatheringAPI = func() *webrtc.API {
	var settings webrtc.SettingEngine
	settings.SetSTUNGatherTimeout(stunGatherTimeout)
	return webrtc.NewAPI(webrtc.WithSettingEngine(settings))
}()

// stunOnly returns a copy of credentials that keeps only STUN URLs, or nil for nil. Xbox issues
// one ICE server, relay.communication.microsoft.com, with a STUN and a TURN URL. Its TURN server
// answers allocations with 300 Try Alternate, which pion's TURN client does not follow, so the
// TURN URL never yields a relay candidate; it only holds candidate gathering open for about 8 s
// before the offer can be sent. Dropping it also means no join is relayed through Microsoft's
// servers, whose addresses servers' VPN checks flag, so a server always sees the player's own
// address.
func stunOnly(credentials *nethernet.Credentials) *nethernet.Credentials {
	if credentials == nil {
		return nil
	}
	filtered := *credentials
	filtered.ICEServers = nil
	for _, server := range credentials.ICEServers {
		urls := slices.DeleteFunc(slices.Clone(server.URLs), func(url string) bool {
			scheme, _, _ := strings.Cut(url, ":")
			return !strings.EqualFold(scheme, "stun") && !strings.EqualFold(scheme, "stuns")
		})
		if len(urls) == 0 {
			continue
		}
		// STUN binding requests carry no credentials.
		filtered.ICEServers = append(filtered.ICEServers, nethernet.ICEServer{URLs: urls})
	}
	return &filtered
}

// stunOnlySignaling is Xbox signaling whose ICE servers are filtered by stunOnly, for joins to
// Realms, friends' worlds and raw NetherNet targets.
type stunOnlySignaling struct{ minecraft.SignalingConn }

// Credentials returns the signaling service's ICE servers with only their STUN URLs.
func (s stunOnlySignaling) Credentials(ctx context.Context) (*nethernet.Credentials, error) {
	credentials, err := s.SignalingConn.Credentials(ctx)
	if err != nil {
		return nil, err
	}
	return stunOnly(credentials), nil
}
