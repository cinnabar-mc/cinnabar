package proxy

import "github.com/pion/webrtc/v4"

// vanillaSCTPReceiveWindow is the SCTP receive buffer of vanilla's NetherNet transport: libwebrtc's
// DcSctpTransport leaves dcsctp's max_receiver_window_buffer_size at its 5 MiB default. A receiver
// completes only messages that fit its buffer. With pion's 1 MiB default, one larger SCTP message
// from a server, which vanilla receives, stalls the connection with the association still open.
const vanillaSCTPReceiveWindow = 5 << 20

// netherNetSettings is the WebRTC configuration shared by every NetherNet connection the core makes.
func netherNetSettings() webrtc.SettingEngine {
	var settings webrtc.SettingEngine
	settings.SetSCTPMaxReceiveBufferSize(vanillaSCTPReceiveWindow)
	return settings
}

// netherNetAPI is the WebRTC API for NetherNet connections to remote servers and friends.
func netherNetAPI() *webrtc.API {
	return webrtc.NewAPI(webrtc.WithSettingEngine(netherNetSettings()))
}
