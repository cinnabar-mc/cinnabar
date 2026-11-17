package conn

import (
	"context"
	"errors"
	"net"
	"testing"
)

func TestSplitAddress(t *testing.T) {
	cases := []struct {
		address string
		host    string
		port    int
		ok      bool
	}{
		{"example.org", "example.org", 25565, true},
		{"example.org:25566", "example.org", 25566, true},
		{"example.org:", "example.org", 25565, true},
		{"example.org: 25570 ", "example.org", 25570, true},
		{"example.org:port", "example.org", 25565, true},
		{"example.org:99999", "example.org", 99999, true},
		{"example.org:-1", "example.org", -1, true},
		{"example.org:99999999999", "example.org", 25565, true},
		{"10.0.0.1:1234", "10.0.0.1", 1234, true},
		{"[::1]", "::1", 25565, true},
		{"[::1]:1234", "::1", 1234, true},
		{"[::1] :1234", "::1", 1234, true},
		{"[::1]:", "::1", 25565, true},
		{"[::1]junk", "::1", 25565, true},
		{"::1", "::1", 25565, true},
		{"a:b:c", "a:b:c", 25565, true},
		{":25566", "", 25566, true},
		{"", "", 25565, true},
		{":", "", 0, false},
		{"::", "", 0, false},
	}
	for _, tc := range cases {
		host, port, err := SplitAddress(tc.address)
		if (err == nil) != tc.ok {
			t.Errorf("SplitAddress(%q): err = %v, want ok=%v", tc.address, err, tc.ok)
			continue
		}
		if tc.ok && (host != tc.host || port != tc.port) {
			t.Errorf("SplitAddress(%q) = %q, %d; want %q, %d", tc.address, host, port, tc.host, tc.port)
		}
	}
}

// srvResolver answers SRV lookups from a fixed table and records the names asked for.
type srvResolver struct {
	records map[string][]*net.SRV
	asked   []string
}

func (r *srvResolver) LookupSRV(_ context.Context, service, proto, name string) (string, []*net.SRV, error) {
	r.asked = append(r.asked, "_"+service+"._"+proto+"."+name)
	if records, ok := r.records[name]; ok {
		return "", records, nil
	}
	return "", nil, errors.New("no such host")
}

func TestResolveAddress(t *testing.T) {
	records := map[string][]*net.SRV{
		"example.org": {{Target: "mc.example.org.", Port: 25570}, {Target: "backup.example.org.", Port: 1}},
	}
	cases := []struct {
		address string
		want    Address
		asked   string
	}{
		{"example.org", Address{Host: "mc.example.org.", Port: 25570}, "_minecraft._tcp.example.org"},
		{"example.org:25565", Address{Host: "mc.example.org.", Port: 25570}, "_minecraft._tcp.example.org"},
		{"example.org:25566", Address{Host: "example.org", Port: 25566}, ""},
		{"other.example", Address{Host: "other.example", Port: 25565}, "_minecraft._tcp.other.example"},
		{"127.0.0.1", Address{Host: "127.0.0.1", Port: 25565}, ""},
		{"[::1]", Address{Host: "::1", Port: 25565}, ""},
	}
	for _, tc := range cases {
		resolver := &srvResolver{records: records}
		got, err := ResolveAddress(context.Background(), resolver, tc.address)
		if err != nil || got != tc.want {
			t.Errorf("ResolveAddress(%q) = %+v, %v; want %+v", tc.address, got, err, tc.want)
		}
		asked := ""
		if len(resolver.asked) > 0 {
			asked = resolver.asked[0]
		}
		if asked != tc.asked || len(resolver.asked) > 1 {
			t.Errorf("ResolveAddress(%q) looked up %q, want %q", tc.address, resolver.asked, tc.asked)
		}
	}
}

func TestResolveAddressRejectsUnusablePorts(t *testing.T) {
	for _, address := range []string{"example.org:99999", "example.org:-1", ":"} {
		if _, err := ResolveAddress(context.Background(), &srvResolver{}, address); err == nil {
			t.Errorf("ResolveAddress(%q) succeeded", address)
		}
	}
}

func TestResolveAddressStopsWhenContextEnds(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, err := ResolveAddress(ctx, &srvResolver{}, "example.org")
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("err = %v, want context.Canceled", err)
	}
}

func TestAddressString(t *testing.T) {
	if got := (Address{Host: "::1", Port: 25565}).String(); got != "[::1]:25565" {
		t.Fatalf("String = %q", got)
	}
}
