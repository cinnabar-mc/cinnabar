package skins

import (
	"bytes"
	"context"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/x509"
	"encoding/hex"
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

// SessionKeySHA256 is the hex SHA-256 of the DER public key the 1.7.10 client bundles to
// check textures signatures with (authlib 1.5.21's yggdrasil_session_pubkey.der). Of the keys
// the keys service lists, only this one is trusted, as the 1.7.10 client trusts only its own.
const SessionKeySHA256 = "4a3f31581c5b9bce0e53dd79a98d2da53fa5e116c3a16c3c36db991f152b2554"

// Config configures a Fetcher. Zero values select the session service, its 1.7.10 signing
// key and http.DefaultClient.
type Config struct {
	HTTP       *http.Client
	KeysURL    string
	ProfileURL string
	// KeySHA256 is the hex SHA-256 of the only listed DER key trusted to sign textures; empty
	// selects SessionKeySHA256.
	KeySHA256 string
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
// err are set. waiters counts the callers waiting for it, under Fetcher.mu, and cancel ends
// the download once none is left.
type call[T any] struct {
	done    chan struct{}
	value   T
	err     error
	waiters int
	ctx     context.Context
	cancel  context.CancelFunc
}

// newCall starts a call with no waiters.
func newCall[T any]() *call[T] {
	ctx, cancel := context.WithCancel(context.Background())
	return &call[T]{done: make(chan struct{}), ctx: ctx, cancel: cancel}
}

// finish records the call's result and wakes its waiters.
func (c *call[T]) finish(value T, err error) {
	c.value, c.err = value, err
	c.cancel()
	close(c.done)
}

// await joins c, which the caller found under f.mu and now releases, and returns its result,
// or ctx's error if ctx ends first. The last waiter to leave an unfinished call cancels it and
// runs drop, with f.mu held, so later callers start a new download instead of joining it.
func await[T any](f *Fetcher, c *call[T], ctx context.Context, drop func()) (T, error) {
	c.waiters++
	f.mu.Unlock()
	select {
	case <-c.done:
		return c.value, c.err
	case <-ctx.Done():
	}
	f.mu.Lock()
	c.waiters--
	if c.waiters == 0 {
		select {
		case <-c.done:
		default:
			c.cancel()
			drop()
		}
	}
	f.mu.Unlock()
	var zero T
	return zero, ctx.Err()
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
	if config.KeySHA256 == "" {
		config.KeySHA256 = SessionKeySHA256
	}
	client := http.DefaultClient
	if config.HTTP != nil {
		client = config.HTTP
	}
	checked := *client
	// A redirect stays on HTTPS and on a texture host or the host first asked.
	checked.CheckRedirect = func(next *http.Request, via []*http.Request) error {
		if len(via) > maxRedirects {
			return errors.New("java edition textures: too many redirects")
		}
		if next.URL.Scheme != "https" || (!textureDomain(next.URL.String()) && next.URL.Host != via[0].URL.Host) {
			return fmt.Errorf("java edition textures: redirect to %s refused", next.URL.Host)
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
	return TexturesKey(properties) != ""
}

// TexturesKey identifies the textures property the 1.7.10 client would read from properties,
// its value and signature, or returns "" when there is none. Equal keys name the same
// textures.
func TexturesKey(properties []java.ProfileProperty) string {
	for _, property := range properties {
		if property.Name == texturesProperty {
			return property.Value + "\x00" + property.Signature
		}
	}
	return ""
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
// A failed download is shared by callers until keysRetryDelay passes; one abandoned by every
// caller may be retried at once.
func (f *Fetcher) signingKeys(ctx context.Context) ([]*rsa.PublicKey, error) {
	f.mu.Lock()
	current := f.keys
	if current == nil || f.failedKeys(current) {
		current = newCall[[]*rsa.PublicKey]()
		f.keys = current
		go func() {
			keys, err := f.downloadKeys(current.ctx)
			if err != nil && current.ctx.Err() == nil {
				f.mu.Lock()
				f.keysRetry = f.config.Now().Add(keysRetryDelay)
				f.mu.Unlock()
			}
			current.finish(keys, err)
		}()
	}
	return await(f, current, ctx, func() {
		if f.keys == current {
			f.keys = nil
		}
	})
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
func (f *Fetcher) downloadKeys(ctx context.Context) ([]*rsa.PublicKey, error) {
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
		if sum := sha256.Sum256(der); err != nil || hex.EncodeToString(sum[:]) != f.config.KeySHA256 {
			continue
		}
		parsed, err := x509.ParsePKIXPublicKey(der)
		if key, ok := parsed.(*rsa.PublicKey); err == nil && ok {
			keys = append(keys, key)
		}
	}
	if len(keys) == 0 {
		return nil, errors.New("java edition signing keys: the service did not list the session key")
	}
	return keys, nil
}

// texture returns the prepared texture at key, downloading it unless it is cached or being
// downloaded. A failed or abandoned download is forgotten, so a later request tries again.
func (f *Fetcher) texture(ctx context.Context, key textureKey) (*image.NRGBA, error) {
	f.mu.Lock()
	current, ok := f.textures[key]
	if !ok {
		current = newCall[*image.NRGBA]()
		f.textures[key] = current
		f.order = append(f.order, key)
		if len(f.order) > maxCachedTextures {
			delete(f.textures, f.order[0])
			f.order = f.order[1:]
		}
		go func() {
			texture, err := f.downloadTexture(current.ctx, key)
			if err != nil {
				f.mu.Lock()
				f.forget(key, current)
				f.mu.Unlock()
			}
			current.finish(texture, err)
		}()
	}
	return await(f, current, ctx, func() { f.forget(key, current) })
}

// forget drops a download from the cache unless a newer one replaced it; f.mu is held.
func (f *Fetcher) forget(key textureKey, dropped *call[*image.NRGBA]) {
	if f.textures[key] != dropped {
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
func (f *Fetcher) downloadTexture(ctx context.Context, key textureKey) (*image.NRGBA, error) {
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
// fetchTimeout bounds the request from when it holds a slot, so waiting for one does not use
// up its time; slot holders are bounded, so the wait ends.
func (f *Fetcher) get(ctx context.Context, address string) ([]byte, error) {
	select {
	case f.slots <- struct{}{}:
		defer func() { <-f.slots }()
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	ctx, cancel := context.WithTimeout(ctx, fetchTimeout)
	defer cancel()
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
