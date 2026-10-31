module github.com/hashimthearab/rust-mcbe/tools/registrygen

go 1.26.1

require (
	github.com/cinnabar-mc/protocolgen/generated/data v0.0.0-20261028003444-2aaf70061542
	github.com/df-mc/dragonfly v0.11.5
	github.com/sandertv/gophertunnel v1.62.0
	github.com/segmentio/fasthash v1.0.3
)

require (
	github.com/brentp/intintmap v0.0.0-20251106190759-56907b1f8479 // indirect
	github.com/df-mc/goleveldb v1.1.9 // indirect
	github.com/df-mc/worldupgrader v1.0.22 // indirect
	github.com/go-gl/mathgl v1.2.0 // indirect
	github.com/golang/snappy v0.0.4 // indirect
	github.com/google/uuid v1.6.0 // indirect
	github.com/klauspost/compress v1.18.4 // indirect
	github.com/onsi/ginkgo v1.16.5 // indirect
	github.com/onsi/gomega v1.17.0 // indirect
	golang.org/x/exp v0.0.0-20260611194520-c48552f49976 // indirect
	golang.org/x/net v0.56.0 // indirect
	golang.org/x/text v0.38.0 // indirect
)

replace github.com/df-mc/dragonfly => github.com/cinnabar-mc/dragonfly v0.0.0-20261028135505-5e4c2e16fb50

replace github.com/sandertv/gophertunnel => github.com/hashimthearab/gophertunnel v1.25.3-0.20261028172540-f8448d729edd
