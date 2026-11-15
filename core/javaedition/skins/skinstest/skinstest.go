// Package skinstest serves fake session and texture services for tests of Java Edition skin
// downloads.
package skinstest

import (
	"bytes"
	"crypto"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha1"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"image"
	"image/color"
	"image/png"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
	"testing"

	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/skins"
)

// Hosts the fake services answer for.
const (
	TextureHost = "textures.minecraft.net"
	KeysHost    = "keys.test"
)

// Services is a fake keys service, session service and texture host behind one test server.
// Every request the fetcher makes reaches it whatever host it names.
type Services struct {
	t      testing.TB
	server *httptest.Server
	key    *rsa.PrivateKey

	mu        sync.Mutex
	files     map[string][]byte
	redirects map[string]string
	requests  []string
	block     chan struct{}
}

// Start serves fake services until the test ends.
func Start(t testing.TB) *Services {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	s := &Services{t: t, key: key, files: map[string][]byte{}, redirects: map[string]string{}}
	der, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatal(err)
	}
	keys, _ := json.Marshal(map[string]any{"profilePropertyKeys": []map[string]string{{"publicKey": base64.StdEncoding.EncodeToString(der)}}})
	s.files[KeysHost+"/publickeys"] = keys
	s.server = httptest.NewServer(http.HandlerFunc(s.serve))
	t.Cleanup(s.server.Close)
	return s
}

// Fetcher returns a fetcher whose every request reaches the fake services.
func (s *Services) Fetcher() *skins.Fetcher {
	return skins.New(skins.Config{
		HTTP:    &http.Client{Transport: redirect{s}},
		KeysURL: "https://" + KeysHost + "/publickeys",
	})
}

// redirect sends every request to the test server, keeping the host it named.
type redirect struct{ s *Services }

func (r redirect) RoundTrip(request *http.Request) (*http.Response, error) {
	r.s.mu.Lock()
	r.s.requests = append(r.s.requests, request.URL.Scheme+"://"+request.URL.Host+request.URL.RequestURI())
	block := r.s.block
	r.s.mu.Unlock()
	if block != nil {
		select {
		case <-block:
		case <-request.Context().Done():
			return nil, request.Context().Err()
		}
	}
	routed := request.Clone(request.Context())
	routed.Host = request.URL.Host
	routed.URL.Scheme = "http"
	routed.URL.Host = strings.TrimPrefix(r.s.server.URL, "http://")
	return http.DefaultTransport.RoundTrip(routed)
}

func (s *Services) serve(w http.ResponseWriter, r *http.Request) {
	s.mu.Lock()
	data, ok := s.files[r.Host+r.URL.Path]
	target, redirected := s.redirects[r.Host+r.URL.Path]
	s.mu.Unlock()
	if redirected {
		http.Redirect(w, r, target, http.StatusFound)
		return
	}
	if !ok {
		http.NotFound(w, r)
		return
	}
	_, _ = w.Write(data)
}

// Block holds every request until the returned function is called.
func (s *Services) Block() (release func()) {
	s.mu.Lock()
	defer s.mu.Unlock()
	block := make(chan struct{})
	s.block = block
	var once sync.Once
	return func() {
		once.Do(func() {
			s.mu.Lock()
			s.block = nil
			s.mu.Unlock()
			close(block)
		})
	}
}

// Requests returns every request made so far, as scheme, host and path with query.
func (s *Services) Requests() []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]string(nil), s.requests...)
}

// Serve makes address, "host/path", answer with data.
func (s *Services) Serve(address string, data []byte) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.files[address] = data
}

// Redirect makes address, "host/path", redirect to target.
func (s *Services) Redirect(address, target string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.redirects[address] = target
}

// Texture serves a w by h PNG filled with fill at http://textures.minecraft.net/texture/hash
// and returns that address.
func (s *Services) Texture(hash string, w, h int, fill color.NRGBA) string {
	img := image.NewNRGBA(image.Rect(0, 0, w, h))
	for i := 0; i < len(img.Pix); i += 4 {
		img.Pix[i], img.Pix[i+1], img.Pix[i+2], img.Pix[i+3] = fill.R, fill.G, fill.B, fill.A
	}
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, img); err != nil {
		s.t.Fatal(err)
	}
	s.Serve(TextureHost+"/texture/"+hash, encoded.Bytes())
	return "http://" + TextureHost + "/texture/" + hash
}

// Payload is the base64 textures payload naming skin and cape; an empty address is left out.
func Payload(skin, cape string) string {
	textures := map[string]map[string]string{}
	if skin != "" {
		textures["SKIN"] = map[string]string{"url": skin}
	}
	if cape != "" {
		textures["CAPE"] = map[string]string{"url": cape}
	}
	data, _ := json.Marshal(map[string]any{"timestamp": 1, "profileName": "Alex", "textures": textures})
	return base64.StdEncoding.EncodeToString(data)
}

// Signed is the textures property with value, signed by the fake session service.
func (s *Services) Signed(value string) java.ProfileProperty {
	digest := sha1.Sum([]byte(value))
	signature, err := rsa.SignPKCS1v15(rand.Reader, s.key, crypto.SHA1, digest[:])
	if err != nil {
		s.t.Fatal(err)
	}
	return java.ProfileProperty{Name: "textures", Value: value, Signature: base64.StdEncoding.EncodeToString(signature)}
}

// Profile serves the session service's unsigned profile of id, undashed, at
// skins.DefaultProfileURL, with value as its textures property.
func (s *Services) Profile(id, value string) {
	data, _ := json.Marshal(map[string]any{"id": id, "name": "Steve", "properties": []map[string]string{{"name": "textures", "value": value}}})
	endpoint, err := url.Parse(skins.DefaultProfileURL + id)
	if err != nil {
		s.t.Fatal(err)
	}
	s.Serve(endpoint.Host+endpoint.Path, data)
}
