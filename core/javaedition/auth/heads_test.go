package auth

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"image"
	"image/color"
	"image/png"
	"net/http"
	"net/http/httptest"
	"os"
	"sync/atomic"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
)

var (
	faceColor = color.NRGBA{R: 200, G: 150, B: 100, A: 0xff}
	hatColor  = color.NRGBA{R: 20, G: 40, B: 220, A: 0xff}
)

// testSkin is a 64×64 skin with a plain face and a hat layer covering only its top-left
// pixel.
func testSkin() *image.NRGBA {
	skin := image.NewNRGBA(image.Rect(0, 0, 64, 64))
	for y := range headSide {
		for x := range headSide {
			skin.SetNRGBA(skinFace.X+x, skinFace.Y+y, faceColor)
		}
	}
	skin.SetNRGBA(skinHat.X, skinHat.Y, hatColor)
	return skin
}

// skinServer serves Notch's profile, with a skin when skin is not nil, and counts profile
// reads.
func skinServer(t *testing.T, skin image.Image) (Services, *atomic.Int32) {
	t.Helper()
	var reads atomic.Int32
	mux := http.NewServeMux()
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)
	mux.HandleFunc("/profile/", func(w http.ResponseWriter, r *http.Request) {
		reads.Add(1)
		textures := map[string]any{"textures": map[string]any{}}
		if skin != nil {
			textures["textures"] = map[string]any{"SKIN": map[string]string{"url": server.URL + "/skin.png"}}
		}
		value, _ := json.Marshal(textures)
		_ = json.NewEncoder(w).Encode(map[string]any{
			"id": notchID, "name": "Notch",
			"properties": []map[string]string{{"name": "textures", "value": base64.StdEncoding.EncodeToString(value)}},
		})
	})
	mux.HandleFunc("/skin.png", func(w http.ResponseWriter, r *http.Request) {
		_ = png.Encode(w, skin)
	})
	return Services{HTTP: server.Client(), SkinProfileURL: server.URL + "/profile/"}, &reads
}

// The head picture is the face with the hat layer's opaque pixels over it, enlarged without
// blurring.
func TestHeadPictureDrawsTheHatOverTheFace(t *testing.T) {
	picture, err := headPicture(testSkin())
	if err != nil {
		t.Fatal(err)
	}
	side := headSide * headPictureScale
	if picture.Bounds() != image.Rect(0, 0, side, side) {
		t.Fatalf("picture bounds = %v", picture.Bounds())
	}
	if got := picture.NRGBAAt(headPictureScale-1, headPictureScale-1); got != hatColor {
		t.Fatalf("hat pixel = %v", got)
	}
	if got := picture.NRGBAAt(headPictureScale, 0); got != faceColor {
		t.Fatalf("face pixel = %v", got)
	}
}

// Old skins fill the hat layer with an opaque colour, which is left out, as is a face's
// transparency.
func TestHeadPictureLeavesOutAFilledHat(t *testing.T) {
	skin := image.NewNRGBA(image.Rect(0, 0, 64, 32))
	for y := range headSide {
		for x := range headSide {
			skin.SetNRGBA(skinFace.X+x, skinFace.Y+y, color.NRGBA{R: faceColor.R, G: faceColor.G, B: faceColor.B})
			skin.SetNRGBA(skinHat.X+x, skinHat.Y+y, hatColor)
		}
	}
	picture, err := headPicture(skin)
	if err != nil {
		t.Fatal(err)
	}
	if got := picture.NRGBAAt(0, 0); got != faceColor {
		t.Fatalf("head pixel = %v, want the opaque face", got)
	}
	if _, err := headPicture(image.NewNRGBA(image.Rect(0, 0, 32, 32))); err == nil {
		t.Fatal("a skin of the wrong size made a picture")
	}
}

// An account's picture is read in the background once and kept, and removing the account
// deletes it.
func TestAccountsReadAndKeepAHeadPicture(t *testing.T) {
	services, reads := skinServer(t, testSkin())
	accounts := newTestAccounts(t, newFakeApp(), services)
	saveAccounts(t, accounts, notchID, Account{ID: notchID, Name: "Notch"})
	if path := accounts.HeadPath(notchID); path != "" {
		t.Fatalf("picture before it was read = %q", path)
	}
	testwait.Eventually(t, 5*time.Second, "the picture to be read", func() bool { return !accounts.HeadsPending() })
	path := accounts.HeadPath(notchID)
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("picture %q: %v", path, err)
	}
	if _, err := png.Decode(bytes.NewReader(data)); err != nil {
		t.Fatalf("picture is not a PNG: %v", err)
	}
	if accounts.HeadsPending() || reads.Load() != 1 {
		t.Fatalf("a fresh picture was read again: %d reads", reads.Load())
	}
	if err := accounts.Remove(context.Background(), notchID); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(path); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("the removed account's picture remains: %v", err)
	}
}

// A profile wearing a default skin has no picture, and is not asked about again at once.
func TestAccountsDefaultSkinHasNoPicture(t *testing.T) {
	services, reads := skinServer(t, nil)
	accounts := newTestAccounts(t, newFakeApp(), services)
	saveAccounts(t, accounts, notchID, Account{ID: notchID, Name: "Notch"})
	accounts.HeadPath(notchID)
	testwait.Eventually(t, 5*time.Second, "the profile to be read", func() bool { return !accounts.HeadsPending() })
	if path := accounts.HeadPath(notchID); path != "" || accounts.HeadsPending() {
		t.Fatalf("default skin picture = %q, pending %v", path, accounts.HeadsPending())
	}
	if reads.Load() != 1 {
		t.Fatalf("profile read %d times", reads.Load())
	}
}
