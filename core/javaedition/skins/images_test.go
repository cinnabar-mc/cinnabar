package skins

import (
	"image"
	"image/color"
	"math"
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

// Each 64x32 texel takes the source texel under its centre, as the 1.7.10 client's nearest
// filtering samples a cape uploaded at its own size: for larger, smaller and
// non-power-of-two capes alike, not the texel at the cell's left or top edge.
func TestLegacyCapeSamplesTexelCentres(t *testing.T) {
	for _, size := range []image.Point{{128, 64}, {22, 17}, {96, 48}, {40, 20}} {
		// Each source texel records its own coordinates.
		source := image.NewNRGBA(image.Rect(0, 0, size.X, size.Y))
		for y := range size.Y {
			for x := range size.X {
				source.SetNRGBA(x, y, color.NRGBA{R: uint8(x), G: uint8(y), A: 0xff})
			}
		}
		cape := legacyCape(source)
		for y := range TextureHeight {
			for x := range TextureWidth {
				want := color.NRGBA{
					R: uint8(math.Floor((float64(x) + 0.5) * float64(size.X) / TextureWidth)),
					G: uint8(math.Floor((float64(y) + 0.5) * float64(size.Y) / TextureHeight)),
					A: 0xff,
				}
				if got := cape.NRGBAAt(x, y); got != want {
					t.Fatalf("%dx%d cape: texel (%d, %d) shows source (%d, %d), want (%d, %d)",
						size.X, size.Y, x, y, got.R, got.G, want.R, want.G)
				}
			}
		}
	}
}
