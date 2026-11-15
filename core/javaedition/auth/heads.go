package auth

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"image/draw"
	"image/png"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"time"
)

// OperationSkin names reading a profile's skin in a StatusError.
const OperationSkin = "skin"

// defaultSkinProfileURL is the session service's public profile endpoint; the profile id
// follows it.
const defaultSkinProfileURL = "https://sessionserver.mojang.com/session/minecraft/profile/"

// Head pictures: a skin's 8×8 face with its hat layer, drawn headPictureScale times larger
// without blurring, kept as "<id>-head.png" beside the account's token.
const (
	headSide         = 8
	headPictureScale = 8
	headPictureFile  = "-head.png"
	// headMaxAge is how long a picture is kept before the skin is read again, so a changed
	// skin shows within a day.
	headMaxAge = 24 * time.Hour
	// headRetryDelay is how long a failed read waits before it is tried again.
	headRetryDelay = 10 * time.Minute
	// headFetchTimeout bounds reading one skin.
	headFetchTimeout = 30 * time.Second
)

// Where a skin texture keeps the face and the hat layer drawn over it.
var (
	skinFace = image.Pt(8, 8)
	skinHat  = image.Pt(40, 8)
)

// errNoSkin reports a profile that wears a default skin, which has no texture to read.
var errNoSkin = errors.New("the Java Edition profile wears a default skin")

// headFetch is the state of an account's picture: being read, or when the last read failed.
type headFetch struct {
	running bool
	failed  time.Time
}

// SkinURL returns the address of the profile's skin texture. A profile wearing a default skin
// has none, and returns errNoSkin.
func (s Services) SkinURL(ctx context.Context, id string) (string, error) {
	var profile struct {
		Properties []struct {
			Name  string `json:"name"`
			Value string `json:"value"`
		} `json:"properties"`
	}
	endpoint := orDefault(s.SkinProfileURL, defaultSkinProfileURL) + url.PathEscape(id)
	if err := s.call(ctx, OperationSkin, http.MethodGet, endpoint, "", nil, &profile); err != nil {
		return "", err
	}
	for _, property := range profile.Properties {
		if property.Name != "textures" {
			continue
		}
		data, err := base64.StdEncoding.DecodeString(property.Value)
		if err != nil {
			return "", fmt.Errorf("java edition %s: malformed textures", OperationSkin)
		}
		var textures struct {
			Textures struct {
				Skin struct {
					URL string `json:"url"`
				} `json:"SKIN"`
			} `json:"textures"`
		}
		if err := json.Unmarshal(data, &textures); err != nil {
			return "", fmt.Errorf("java edition %s: malformed textures", OperationSkin)
		}
		if textures.Textures.Skin.URL == "" {
			return "", errNoSkin
		}
		return textures.Textures.Skin.URL, nil
	}
	return "", errNoSkin
}

// Skin downloads the skin texture at address.
func (s Services) Skin(ctx context.Context, address string) (image.Image, error) {
	parsed, err := url.Parse(address)
	if err != nil || (parsed.Scheme != "https" && parsed.Scheme != "http") {
		return nil, fmt.Errorf("java edition %s: unsupported texture address", OperationSkin)
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, address, nil)
	if err != nil {
		return nil, err
	}
	client := s.HTTP
	if client == nil {
		client = http.DefaultClient
	}
	response, err := client.Do(request)
	if err != nil {
		return nil, fmt.Errorf("java edition %s: %w", OperationSkin, err)
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode > 299 {
		return nil, &StatusError{Operation: OperationSkin, Status: response.StatusCode}
	}
	data, err := io.ReadAll(io.LimitReader(response.Body, maxResponseBytes))
	if err != nil {
		return nil, fmt.Errorf("java edition %s: %w", OperationSkin, err)
	}
	skin, err := png.Decode(bytes.NewReader(data))
	if err != nil {
		return nil, fmt.Errorf("java edition %s: malformed texture", OperationSkin)
	}
	return skin, nil
}

// headPicture draws a skin's face with its hat layer over it. The face is opaque, and a hat
// layer with no transparent pixel, as old skins fill it, is left out.
func headPicture(skin image.Image) (*image.NRGBA, error) {
	bounds := skin.Bounds()
	if bounds.Dx() != 64 || (bounds.Dy() != 32 && bounds.Dy() != 64) {
		return nil, fmt.Errorf("java edition skin is %dx%d, not 64x32 or 64x64", bounds.Dx(), bounds.Dy())
	}
	face := image.NewNRGBA(image.Rect(0, 0, headSide, headSide))
	draw.Draw(face, face.Bounds(), skin, bounds.Min.Add(skinFace), draw.Src)
	for i := 3; i < len(face.Pix); i += 4 {
		face.Pix[i] = 0xff
	}
	hat := image.NewNRGBA(face.Bounds())
	draw.Draw(hat, hat.Bounds(), skin, bounds.Min.Add(skinHat), draw.Src)
	if hasTransparency(hat) {
		draw.Draw(face, face.Bounds(), hat, image.Point{}, draw.Over)
	}
	side := headSide * headPictureScale
	picture := image.NewNRGBA(image.Rect(0, 0, side, side))
	for y := range side {
		for x := range side {
			picture.SetNRGBA(x, y, face.NRGBAAt(x/headPictureScale, y/headPictureScale))
		}
	}
	return picture, nil
}

func hasTransparency(layer *image.NRGBA) bool {
	for i := 3; i < len(layer.Pix); i += 4 {
		if layer.Pix[i] != 0xff {
			return true
		}
	}
	return false
}

// HeadPath returns the account's head picture, or "" while it has none. A missing or old
// picture is read in the background; HeadsPending reports reads under way.
func (a *Accounts) HeadPath(id string) string {
	if !profileIDPattern.MatchString(id) {
		return ""
	}
	path := a.headPath(id)
	info, err := os.Stat(path)
	if err != nil || a.config.Now().Sub(info.ModTime()) > headMaxAge {
		a.fetchHead(id)
	}
	if err != nil {
		return ""
	}
	return path
}

// HeadsPending reports whether a head picture is being read, so the caller can ask again
// once it is.
func (a *Accounts) HeadsPending() bool {
	a.mu.Lock()
	defer a.mu.Unlock()
	for _, fetch := range a.heads {
		if fetch.running {
			return true
		}
	}
	return false
}

// fetchHead reads the account's head picture in the background, unless it is being read or
// its last read failed recently.
func (a *Accounts) fetchHead(id string) {
	a.mu.Lock()
	fetch := a.heads[id]
	if a.closed || fetch.running || (!fetch.failed.IsZero() && a.config.Now().Sub(fetch.failed) < headRetryDelay) {
		a.mu.Unlock()
		return
	}
	a.heads[id] = headFetch{running: true}
	a.work.Add(1)
	a.mu.Unlock()
	go func() {
		defer a.work.Done()
		err := a.saveHead(id)
		a.mu.Lock()
		defer a.mu.Unlock()
		var failed time.Time
		if err != nil {
			failed = a.config.Now()
			if !errors.Is(err, errNoSkin) {
				a.config.Logger.Debug("Java Edition account picture not read", "error", err)
			}
		}
		a.heads[id] = headFetch{failed: failed}
	}()
}

// saveHead reads the account's skin and saves its head picture, unless the account was
// removed meanwhile.
func (a *Accounts) saveHead(id string) error {
	ctx, cancel := context.WithTimeout(a.lifetime, headFetchTimeout)
	defer cancel()
	address, err := a.config.Services.SkinURL(ctx, id)
	if err != nil {
		return err
	}
	skin, err := a.config.Services.Skin(ctx, address)
	if err != nil {
		return err
	}
	picture, err := headPicture(skin)
	if err != nil {
		return err
	}
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, picture); err != nil {
		return err
	}
	index, err := a.readIndex()
	if err != nil {
		return err
	}
	if _, ok := index.find(id); !ok {
		return ErrNoAccount
	}
	return writeFileAtomic(a.headPath(id), encoded.Bytes())
}

func (a *Accounts) headPath(id string) string {
	return filepath.Join(a.config.Dir, id+headPictureFile)
}
