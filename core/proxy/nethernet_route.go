package proxy

import (
	"fmt"
	"log/slog"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/redact"
	"github.com/pion/ice/v4"
	"github.com/pion/logging"
	"github.com/pion/webrtc/v4"
)

// selectedPairFormat is the trace message pion's ICE agent writes, with the pair as its only
// argument, whenever it selects the candidate pair a connection sends through.
const selectedPairFormat = "Set selected candidate pair: %s"

// remoteNetherNetAPI dials remote NetherNet servers with pion's defaults and logs the ICE route
// each connection selects, so a high ping can be traced to a slow path or a slow measurement.
func remoteNetherNetAPI(logger *slog.Logger) *webrtc.API {
	var settings webrtc.SettingEngine
	settings.LoggerFactory = routeLoggerFactory{log: logger, LoggerFactory: logging.NewDefaultLoggerFactory()}
	return webrtc.NewAPI(webrtc.WithSettingEngine(settings))
}

// routeLoggerFactory keeps pion's default logging and adds route reporting to the ICE agent.
type routeLoggerFactory struct {
	logging.LoggerFactory
	log *slog.Logger
}

// NewLogger returns pion's default logger for scope, reporting selected pairs for "ice".
func (factory routeLoggerFactory) NewLogger(scope string) logging.LeveledLogger {
	base := factory.LoggerFactory.NewLogger(scope)
	if scope != "ice" {
		return base
	}
	return routeLogger{LeveledLogger: base, log: factory.log}
}

// routeLogger reports selected candidate pairs and forwards every message to pion's logger.
type routeLogger struct {
	logging.LeveledLogger
	log *slog.Logger
}

// Tracef logs the route when format is the selected-pair message, then forwards the message.
func (logger routeLogger) Tracef(format string, args ...any) {
	if format == selectedPairFormat && len(args) == 1 {
		if pair, ok := args[0].(*ice.CandidatePair); ok && pair != nil {
			logRoute(logger.log, pair)
		}
	}
	logger.LeveledLogger.Tracef(format, args...)
}

// logRoute records which kinds of candidate a connection pairs and the round trip ICE measured
// when it selected them. A relay on either side means traffic goes through a TURN server.
func logRoute(log *slog.Logger, pair *ice.CandidatePair) {
	if log == nil {
		log = slog.Default()
	}
	rtt := time.Duration(pair.CurrentRoundTripTime() * float64(time.Second))
	log.Info("NetherNet route selected",
		"local", pair.Local.Type().String(),
		"remote", pair.Remote.Type().String(),
		"network", pair.Local.NetworkType().String(),
		"remote_address", redact.Private(fmt.Sprintf("%s:%d", pair.Remote.Address(), pair.Remote.Port())),
		"rtt_ms", rtt.Milliseconds(),
	)
}
