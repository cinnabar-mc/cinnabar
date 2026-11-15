package skins

import (
	"bytes"
	"context"
	"crypto/rsa"
	"crypto/x509"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"image/png"
	"io"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"

	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
)

// Default service addresses.
const (
	// DefaultKeysURL lists the keys the session service signs profile properties with.
	DefaultKeysURL = "https://api.minecraftservices.com/publickeys"
	// DefaultProfileURL is the session service's profile endpoint; the undashed profile id
	// follows it.
	DefaultProfileURL = "https://sessionserver.mojang.com/session/minecraft/profile/"
)

// Bounds on the work one fetcher does.
const (
	// fetchTimeout bounds one download: the keys, a profile or a texture.
	fetchTimeout = 15 * time.Second
	// keysRetryDelay is how long a failed keys download is reused before it is tried again.
	keysRetryDelay = time.Minute
	// maxResponseBytes bounds a response body; textures are a few kilobytes.
	maxResponseBytes = 256 << 10
	// maxTextureSide bounds a texture's width and height before it is decoded.
	maxTextureSide = 1024
	// maxRedirects bounds the redirects followed by one download.
	maxRedirects = 3
	// maxConcurrent bounds the downloads under way at once.
	maxConcurrent = 4
	// maxCachedTextures bounds the prepared textures kept, about 8 KiB each.
	maxCachedTextures = 256
)

// Config configures a Fetcher. Zero values select the session service and http.DefaultClient.
type Config struct {
	HTTP       *http.Client
	KeysURL    string
	ProfileURL string
	// Now is the clock that schedules retries; nil uses time.Now.
	Now func() time.Time
}

// Images are a player's textures prepared as 64x32 RGBA pixels. A nil Skin is the default
// skin and a nil Cape is no cape. The hashes name the textures, as their addresses do.
type Images struct {
	Skin, Cape         []byte
	SkinHash, CapeHash string
}

// Fetcher downloads and prepares profile textures. Downloads are shared between callers,
// bounded in number, and their results cached by address. It is safe for concurrent use.
type Fetcher struct {
	config Config
	client *http.Client
	slots  chan struct{}

	mu sync.Mutex
	// keys is the last keys download, and keysRetry when a failed one may be retried.
	keys      *call[[]*rsa.PublicKey]
	keysRetry time.Time
	// textures holds downloads by kind and address, oldest first in order.
	textures map[textureKey]*call[*image.NRGBA]
	order    []textureKey
}

// textureKey names a prepared texture: the same image prepares differently as a skin and as
// a cape.
type textureKey struct {
	address string
	cape    bool
}

// call is a download shared by everyone asking for the same thing; done closes once value and
// err are set.
type call[T any] struct {
	done  chan struct{}
	value T
	err   error
}

// wait returns the call's result, or ctx's error if it ends first.
func (c *call[T]) wait(ctx context.Context) (T, error) {
	select {
	case <-c.done:
		return c.value, c.err
	case <-ctx.Done():
		var zero T
		return zero, ctx.Err()
	}
}

// New returns a fetcher with config.
func New(config Config) *Fetcher {
	if config.KeysURL == "" {
		config.KeysURL = DefaultKeysURL
	}
	if config.ProfileURL == "" {
		config.ProfileURL = DefaultProfileURL
	}
	if config.Now == nil {
		config.Now = time.Now
	}
	client := http.DefaultClient
	if config.HTTP != nil {
		client = config.HTTP
	}
	checked := *client
	checked.CheckRedirect = func(next *http.Request, via []*http.Request) error {
		if len(via) > maxRedirects {
			return errors.New("java edition textures: too many redirects")
		}
		return nil
	}
	return &Fetcher{
		config:   config,
		client:   &checked,
		slots:    make(chan struct{}, maxConcurrent),
		textures: map[textureKey]*call[*image.NRGBA]{},
	}
}

// Profile returns the textures another player's profile properties name, as the 1.7.10
// client reads them: only a textures property the session service signed counts. Properties
// naming no textures give empty Images and no error. A texture that fails to download is left
// out and reported in the error alongside the others.
func (f *Fetcher) Profile(ctx context.Context, properties []java.ProfileProperty) (Images, error) {
	if !hasTextures(properties) {
		return Images{}, nil
	}
	keys, err := f.signingKeys(ctx)
	if err != nil {
		return Images{}, err
	}
	textures, err := decodeTextures(properties, true, keys)
	if err != nil {
		return Images{}, err
	}
	return f.images(ctx, textures)
}

// Own returns the textures of the player's own profile, which the 1.7.10 client reads from
// the session service without requiring a signature. id is the profile id with or without
// dashes.
func (f *Fetcher) Own(ctx context.Context, id string) (Images, error) {
	var profile struct {
		Properties []struct {
			Name      string `json:"name"`
			Value     string `json:"value"`
			Signature string `json:"signature"`
		} `json:"properties"`
	}
	endpoint := f.config.ProfileURL + url.PathEscape(strings.ReplaceAll(id, "-", "")) + "?unsigned=true"
	data, err := f.get(ctx, endpoint)
	if err != nil {
		return Images{}, err
	}
	if len(data) == 0 {
		// The service answers an unknown profile with no content.
		return Images{}, nil
	}
	if err := json.Unmarshal(data, &profile); err != nil {
		return Images{}, fmt.Errorf("java edition profile: %w", err)
	}
	properties := make([]java.ProfileProperty, len(profile.Properties))
	for i, property := range profile.Properties {
		properties[i] = java.ProfileProperty{Name: property.Name, Value: property.Value, Signature: property.Signature}
	}
	textures, err := decodeTextures(properties, false, nil)
	if err != nil {
		return Images{}, err
	}
	return f.images(ctx, textures)
}

// hasTextures reports whether properties hold a textures property.
func hasTextures(properties []java.ProfileProperty) bool {
	for _, property := range properties {
		if property.Name == texturesProperty {
			return true
		}
	}
	return false
}

// images downloads and prepares the textures, each independently.
func (f *Fetcher) images(ctx context.Context, textures Textures) (Images, error) {
	var images Images
	var errs []error
	if textures.Skin != "" {
		skin, err := f.texture(ctx, textureKey{address: textures.Skin})
		if err != nil {
			errs = append(errs, fmt.Errorf("skin %s: %w", textures.Skin, err))
		} else {
			images.Skin, images.SkinHash = skin.Pix, textureHash(textures.Skin)
		}
	}
	if textures.Cape != "" {
		cape, err := f.texture(ctx, textureKey{address: textures.Cape, cape: true})
		if err != nil {
			errs = append(errs, fmt.Errorf("cape %s: %w", textures.Cape, err))
		} else {
			images.Cape, images.CapeHash = cape.Pix, textureHash(textures.Cape)
		}
	}
	return images, errors.Join(errs...)
}

// textureHash is the last path element of a texture address, which the texture service
// names by the image's hash.
func textureHash(address string) string {
	parsed, err := url.Parse(address)
	if err != nil {
		return address
	}
	path := strings.TrimSuffix(parsed.Path, "/")
	return path[strings.LastIndex(path, "/")+1:]
}

// signingKeys returns the session service's profile property keys, downloading them once.
// A failed download is shared by callers until keysRetryDelay passes.
func (f *Fetcher) signingKeys(ctx context.Context) ([]*rsa.PublicKey, error) {
	f.mu.Lock()
	current := f.keys
	if current == nil || f.failedKeys(current) {
		current = &call[[]*rsa.PublicKey]{done: make(chan struct{})}
		f.keys = current
		go func() {
			current.value, current.err = f.downloadKeys()
			if current.err != nil {
				f.mu.Lock()
				f.keysRetry = f.config.Now().Add(keysRetryDelay)
				f.mu.Unlock()
			}
			close(current.done)
		}()
	}
	f.mu.Unlock()
	return current.wait(ctx)
}

// failedKeys reports whether a finished keys download failed and may be retried; f.mu is
// held.
func (f *Fetcher) failedKeys(keys *call[[]*rsa.PublicKey]) bool {
	select {
	case <-keys.done:
		return keys.err != nil && !f.config.Now().Before(f.keysRetry)
	default:
		return false
	}
}

// downloadKeys reads the profile property keys from the keys service.
func (f *Fetcher) downloadKeys() ([]*rsa.PublicKey, error) {
	ctx, cancel := context.WithTimeout(context.Background(), fetchTimeout)
	defer cancel()
	data, err := f.get(ctx, f.config.KeysURL)
	if err != nil {
		return nil, err
	}
	var listed struct {
		ProfilePropertyKeys []struct {
			PublicKey string `json:"publicKey"`
		} `json:"profilePropertyKeys"`
	}
	if err := json.Unmarshal(data, &listed); err != nil {
		return nil, fmt.Errorf("java edition signing keys: %w", err)
	}
	var keys []*rsa.PublicKey
	for _, entry := range listed.ProfilePropertyKeys {
		der, err := decodeBase64(entry.PublicKey)
		if err != nil {
			continue
		}
		parsed, err := x509.ParsePKIXPublicKey(der)
		if key, ok := parsed.(*rsa.PublicKey); err == nil && ok {
			keys = append(keys, key)
		}
	}
	if len(keys) == 0 {
		return nil, errors.New("java edition signing keys: the service listed no usable key")
	}
	return keys, nil
}

// texture returns the prepared texture at key, downloading it unless it is cached or being
// downloaded. A failed download is forgotten, so a later request tries again.
func (f *Fetcher) texture(ctx context.Context, key textureKey) (*image.NRGBA, error) {
	f.mu.Lock()
	current, ok := f.textures[key]
	if !ok {
		current = &call[*image.NRGBA]{done: make(chan struct{})}
		f.textures[key] = current
		f.order = append(f.order, key)
		if len(f.order) > maxCachedTextures {
			delete(f.textures, f.order[0])
			f.order = f.order[1:]
		}
		go func() {
			current.value, current.err = f.downloadTexture(key)
			if current.err != nil {
				f.forget(key, current)
			}
			close(current.done)
		}()
	}
	f.mu.Unlock()
	return current.wait(ctx)
}

// forget drops a failed download from the cache unless a newer one replaced it.
func (f *Fetcher) forget(key textureKey, failed *call[*image.NRGBA]) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.textures[key] != failed {
		return
	}
	delete(f.textures, key)
	for i, listed := range f.order {
		if listed == key {
			f.order = append(f.order[:i], f.order[i+1:]...)
			break
		}
	}
}

// downloadTexture reads and prepares one texture. Texture hosts serve HTTPS, so a plain
// address is read over HTTPS.
func (f *Fetcher) downloadTexture(key textureKey) (*image.NRGBA, error) {
	ctx, cancel := context.WithTimeout(context.Background(), fetchTimeout)
	defer cancel()
	address := key.address
	if rest, ok := strings.CutPrefix(address, "http://"); ok {
		address = "https://" + rest
	}
	data, err := f.get(ctx, address)
	if err != nil {
		return nil, err
	}
	config, err := png.DecodeConfig(bytes.NewReader(data))
	if err != nil {
		return nil, fmt.Errorf("not a PNG image: %w", err)
	}
	if config.Width <= 0 || config.Height <= 0 || config.Width > maxTextureSide || config.Height > maxTextureSide {
		return nil, fmt.Errorf("unsupported texture size %dx%d", config.Width, config.Height)
	}
	decoded, err := png.Decode(bytes.NewReader(data))
	if err != nil {
		return nil, fmt.Errorf("not a PNG image: %w", err)
	}
	if key.cape {
		return legacyCape(decoded), nil
	}
	return legacySkin(decoded), nil
}

// get reads a response body of at most maxResponseBytes, holding one of the download slots.
func (f *Fetcher) get(ctx context.Context, address string) ([]byte, error) {
	ctx, cancel := context.WithTimeout(ctx, fetchTimeout)
	defer cancel()
	select {
	case f.slots <- struct{}{}:
		defer func() { <-f.slots }()
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, address, nil)
	if err != nil {
		return nil, err
	}
	response, err := f.client.Do(request)
	if err != nil {
		return nil, err
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode > 299 {
		return nil, fmt.Errorf("%s answered %s", request.URL.Host, response.Status)
	}
	data, err := io.ReadAll(io.LimitReader(response.Body, maxResponseBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > maxResponseBytes {
		return nil, fmt.Errorf("%s sent more than %d bytes", request.URL.Host, maxResponseBytes)
	}
	return data, nil
}
