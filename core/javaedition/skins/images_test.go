package skins

import (
	"image"
	"image/color"
	"testing"
)

func TestLegacySkinFollowsTheSkinBuffer(t *testing.T) {
	source := image.NewNRGBA(image.Rect(0, 0, 64, 64))
	for i := 0; i < len(source.Pix); i += 4 {
		source.Pix[i], source.Pix[i+3] = 0x80, 0xff
	}
	// A see-through head pixel, a hat pixel with some transparency, and a lower-half pixel
	// that the 64x32 texture drops.
	source.SetNRGBA(8, 8, color.NRGBA{R: 0x40, A: 0})
	source.SetNRGBA(40, 8, color.NRGBA{G: 0xff, A: 0x10})
	source.SetNRGBA(0, 40, color.NRGBA{B: 0xff, A: 0xff})

	skin := legacySkin(source)
	if skin.Bounds() != image.Rect(0, 0, 64, 32) {
		t.Fatalf("bounds = %v", skin.Bounds())
	}
	if got := skin.NRGBAAt(8, 8); got != (color.NRGBA{A: 0xff}) {
		t.Fatalf("see-through head pixel = %v, want opaque black", got)
	}
	// The hat already has a transparent pixel, so it is kept as it is.
	if got := skin.NRGBAAt(41, 8); got.A != 0xff {
		t.Fatalf("hat pixel = %v, want it kept opaque", got)
	}
	if got := skin.NRGBAAt(40, 8); got.A != 0x10 {
		t.Fatalf("translucent hat pixel = %v", got)
	}
}

func TestLegacyCapeIsFittedTo64x32(t *testing.T) {
	source := image.NewNRGBA(image.Rect(0, 0, 22, 17))
	source.SetNRGBA(21, 16, color.NRGBA{R: 0xff, A: 0xff})
	cape := legacyCape(source)
	if cape.Bounds() != image.Rect(0, 0, 64, 32) {
		t.Fatalf("bounds = %v", cape.Bounds())
	}
	if got := cape.NRGBAAt(63, 31); got != (color.NRGBA{R: 0xff, A: 0xff}) {
		t.Fatalf("corner = %v", got)
	}
}

// Each 64x32 texel takes the source texel under its centre, as nearest filtering samples it:
// a 128x64 cape shows its odd columns and rows, not its even ones.
func TestLegacyCapeSamplesTexelCentres(t *testing.T) {
	source := image.NewNRGBA(image.Rect(0, 0, 128, 64))
	for y := range 64 {
		for x := range 128 {
			if x%2 == 1 && y%2 == 1 {
				source.SetNRGBA(x, y, color.NRGBA{G: 0xff, A: 0xff})
			} else {
				source.SetNRGBA(x, y, color.NRGBA{R: 0xff, A: 0xff})
			}
		}
	}
	cape := legacyCape(source)
	for _, at := range []image.Point{{0, 0}, {10, 5}, {63, 31}} {
		if got := cape.NRGBAAt(at.X, at.Y); got != (color.NRGBA{G: 0xff, A: 0xff}) {
			t.Fatalf("texel %v = %v, want the source texel under its centre", at, got)
		}
	}
}
