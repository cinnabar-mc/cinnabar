package skins_test

import (
	"context"
	"errors"
	"image/color"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/skins"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/skins/skinstest"
)

var (
	red  = color.NRGBA{R: 0xff, A: 0xff}
	blue = color.NRGBA{B: 0xff, A: 0xff}
)

func testContext(t *testing.T) context.Context {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	t.Cleanup(cancel)
	return ctx
}

// pixel returns the RGBA of a prepared 64-pixel-wide texture at x, y.
func pixel(data []byte, x, y int) color.NRGBA {
	i := (y*skins.TextureWidth + x) * 4
	return color.NRGBA{R: data[i], G: data[i+1], B: data[i+2], A: data[i+3]}
}

func TestSignedTexturesAreDownloadedAndPrepared(t *testing.T) {
	services := skinstest.Start(t)
	skin := services.Texture("skinhash", 64, 64, red)
	cape := services.Texture("capehash", 128, 64, blue)
	images, err := services.Fetcher().Profile(testContext(t), []java.ProfileProperty{services.Signed(skinstest.Payload(skin, cape))})
	if err != nil {
		t.Fatal(err)
	}
	if len(images.Skin) != 64*32*4 || len(images.Cape) != 64*32*4 {
		t.Fatalf("skin is %d bytes and cape %d, want 64x32 RGBA each", len(images.Skin), len(images.Cape))
	}
	if images.SkinHash != "skinhash" || images.CapeHash != "capehash" {
		t.Fatalf("hashes = %q, %q", images.SkinHash, images.CapeHash)
	}
	if got := pixel(images.Skin, 0, 20); got != red {
		t.Fatalf("body pixel = %v", got)
	}
	// An opaque hat layer is cleared, as the 1.7.10 skin buffer clears old skins' hats.
	if got := pixel(images.Skin, 40, 8); got.A != 0 {
		t.Fatalf("hat pixel = %v, want transparent", got)
	}
	if got := pixel(images.Cape, 63, 31); got != blue {
		t.Fatalf("cape pixel = %v", got)
	}
	for _, request := range services.Requests() {
		if strings.Contains(request, skinstest.TextureHost) && !strings.HasPrefix(request, "https://") {
			t.Fatalf("texture read over plain HTTP: %s", request)
		}
	}
}

func TestTexturesAreDownloadedOnce(t *testing.T) {
	services := skinstest.Start(t)
	fetcher := services.Fetcher()
	property := services.Signed(skinstest.Payload(services.Texture("once", 64, 32, red), ""))
	for range 3 {
		if _, err := fetcher.Profile(testContext(t), []java.ProfileProperty{property}); err != nil {
			t.Fatal(err)
		}
	}
	if got := len(services.Requests()); got != 2 {
		t.Fatalf("made %d requests for the keys and one texture: %v", got, services.Requests())
	}
}

func TestUnsignedOrForgedTexturesAreRefused(t *testing.T) {
	services := skinstest.Start(t)
	payload := skinstest.Payload(services.Texture("forged", 64, 32, red), "")
	other := skinstest.Start(t).Signed(payload)
	for name, property := range map[string]java.ProfileProperty{
		"unsigned":           {Name: "textures", Value: payload},
		"another signer":     other,
		"another listed key": services.SignedByOther(payload),
		"altered":            {Name: "textures", Value: skinstest.Payload("http://textures.minecraft.net/texture/other", ""), Signature: other.Signature},
	} {
		images, err := services.Fetcher().Profile(testContext(t), []java.ProfileProperty{property})
		if err == nil || images.Skin != nil {
			t.Fatalf("%s textures were accepted: %v", name, err)
		}
	}
}

func TestTexturesNamingOtherHostsAreIgnored(t *testing.T) {
	services := skinstest.Start(t)
	payload := skinstest.Payload(services.Texture("good", 64, 32, red), "http://example.com/cape.png")
	images, err := services.Fetcher().Profile(testContext(t), []java.ProfileProperty{services.Signed(payload)})
	if err != nil || images.Skin != nil || images.Cape != nil {
		t.Fatalf("images = %+v, %v; want none", images, err)
	}
}

func TestPropertiesWithoutTexturesNameTheDefaultSkin(t *testing.T) {
	services := skinstest.Start(t)
	for name, properties := range map[string][]java.ProfileProperty{
		"none":      nil,
		"other":     {{Name: "other", Value: "x"}},
		"malformed": {services.Signed("not base64!")},
		"not json":  {services.Signed("bm90IGpzb24")},
	} {
		images, err := services.Fetcher().Profile(testContext(t), properties)
		if err != nil || images.Skin != nil || images.Cape != nil {
			t.Fatalf("%s: images = %+v, %v; want the default skin", name, images, err)
		}
	}
}

func TestAFailedCapeKeepsTheSkin(t *testing.T) {
	services := skinstest.Start(t)
	skin := services.Texture("skin", 64, 32, red)
	payload := skinstest.Payload(skin, "http://textures.minecraft.net/texture/missing")
	images, err := services.Fetcher().Profile(testContext(t), []java.ProfileProperty{services.Signed(payload)})
	if err == nil || images.Skin == nil || images.Cape != nil {
		t.Fatalf("images = skin %t, cape %t, error %v", images.Skin != nil, images.Cape != nil, err)
	}
}

func TestOversizedTexturesAreRefused(t *testing.T) {
	services := skinstest.Start(t)
	services.Serve(skinstest.TextureHost+"/texture/huge", make([]byte, 1<<20))
	payload := skinstest.Payload("http://textures.minecraft.net/texture/huge", "")
	images, err := services.Fetcher().Profile(testContext(t), []java.ProfileProperty{services.Signed(payload)})
	if err == nil || images.Skin != nil {
		t.Fatalf("an oversized texture was accepted: %v", err)
	}
}

func TestOwnTexturesAreReadUnsignedFromTheSessionService(t *testing.T) {
	services := skinstest.Start(t)
	const id = "0123456789abcdef0123456789abcdef"
	services.Profile(id, skinstest.Payload(services.Texture("own", 64, 32, red), ""))
	images, err := services.Fetcher().Own(testContext(t), "01234567-89ab-cdef-0123-456789abcdef")
	if err != nil || images.SkinHash != "own" {
		t.Fatalf("images = %+v, %v", images, err)
	}
	want := skins.DefaultProfileURL + id + "?unsigned=true"
	if !slices.Contains(services.Requests(), want) {
		t.Fatalf("requests = %v, want %s", services.Requests(), want)
	}
}

// A download every caller has left is cancelled, so it does not hold a download slot, and a
// later request starts it again.
func TestADownloadEveryCallerLeftIsCancelled(t *testing.T) {
	services := skinstest.Start(t)
	fetcher := services.Fetcher()
	property := services.Signed(skinstest.Payload(services.Texture("left", 64, 32, red), ""))
	release := services.Block()
	defer release()
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() {
		_, err := fetcher.Profile(ctx, []java.ProfileProperty{property})
		done <- err
	}()
	testwait.Eventually(t, testwait.DefaultTimeout, "the keys request", func() bool { return len(services.Requests()) == 1 })
	cancel()
	if err := <-done; !errors.Is(err, context.Canceled) {
		t.Fatalf("err = %v, want the context's", err)
	}
	testwait.Eventually(t, testwait.DefaultTimeout, "the abandoned request to be cancelled", func() bool { return services.Cancelled() == 1 })
	release()
	if images, err := fetcher.Profile(testContext(t), []java.ProfileProperty{property}); err != nil || images.SkinHash != "left" {
		t.Fatalf("a later request = %+v, %v", images, err)
	}
}

func TestTextureRedirectsStayOnTextureHosts(t *testing.T) {
	services := skinstest.Start(t)
	services.Texture("moved", 64, 32, red)
	services.Redirect(skinstest.TextureHost+"/texture/kept", "https://"+skinstest.TextureHost+"/texture/moved")
	services.Redirect(skinstest.TextureHost+"/texture/away", "https://internal.example/texture/moved")
	fetcher := services.Fetcher()
	kept, err := fetcher.Profile(testContext(t), []java.ProfileProperty{services.Signed(skinstest.Payload("http://textures.minecraft.net/texture/kept", ""))})
	if err != nil || kept.Skin == nil {
		t.Fatalf("a redirect within the texture host failed: %v", err)
	}
	away, err := fetcher.Profile(testContext(t), []java.ProfileProperty{services.Signed(skinstest.Payload("http://textures.minecraft.net/texture/away", ""))})
	if err == nil || away.Skin != nil {
		t.Fatal("a redirect to another host was followed")
	}
	for _, request := range services.Requests() {
		if strings.Contains(request, "internal.example") {
			t.Fatalf("requested %s", request)
		}
	}
}

// The pinned hash is that of the key the 1.7.10 client bundles; a fetcher with the default
// configuration trusts no key but that one.
func TestOnlyThePinnedSessionKeyIsTrusted(t *testing.T) {
	services := skinstest.Start(t)
	payload := skinstest.Payload(services.Texture("pinned", 64, 32, red), "")
	fetcher := skins.New(skins.Config{HTTP: services.HTTPClient(), KeysURL: "https://" + skinstest.KeysHost + "/publickeys"})
	if _, err := fetcher.Profile(testContext(t), []java.ProfileProperty{services.Signed(payload)}); err == nil {
		t.Fatal("a key other than the 1.7.10 session key was trusted")
	}
}
