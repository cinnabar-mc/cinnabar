package skins

import (
	"image"
	"image/draw"
)

// TextureWidth and TextureHeight are the size of a 1.7.10 skin and cape texture.
const (
	TextureWidth  = 64
	TextureHeight = 32
)

// legacySkin draws a downloaded skin the way the 1.7.10 client's skin buffer does: its top
// left 64x32 corner over a transparent texture, with the head and body made opaque and the
// hat layer made fully transparent unless some of it already is.
func legacySkin(source image.Image) *image.NRGBA {
	skin := image.NewNRGBA(image.Rect(0, 0, TextureWidth, TextureHeight))
	draw.Draw(skin, skin.Bounds(), source, source.Bounds().Min, draw.Over)
	setOpaque(skin, image.Rect(0, 0, 32, 16))
	hat := image.Rect(32, 0, 64, 32)
	if !hasTransparency(skin, hat) {
		setTransparent(skin, hat)
	}
	setOpaque(skin, image.Rect(0, 16, 64, 32))
	return skin
}

// legacyCape fits a downloaded cape to 64x32 by nearest pixel: the 1.7.10 cape model samples
// its texture in 64x32 texture coordinates whatever the image size.
func legacyCape(source image.Image) *image.NRGBA {
	cape := image.NewNRGBA(image.Rect(0, 0, TextureWidth, TextureHeight))
	bounds := source.Bounds()
	for y := range TextureHeight {
		for x := range TextureWidth {
			sx := bounds.Min.X + x*bounds.Dx()/TextureWidth
			sy := bounds.Min.Y + y*bounds.Dy()/TextureHeight
			cape.Set(x, y, source.At(sx, sy))
		}
	}
	return cape
}

// setOpaque gives every pixel in area full alpha, keeping its colour.
func setOpaque(img *image.NRGBA, area image.Rectangle) {
	for y := area.Min.Y; y < area.Max.Y; y++ {
		for x := area.Min.X; x < area.Max.X; x++ {
			img.Pix[img.PixOffset(x, y)+3] = 0xff
		}
	}
}

// setTransparent gives every pixel in area zero alpha, keeping its colour.
func setTransparent(img *image.NRGBA, area image.Rectangle) {
	for y := area.Min.Y; y < area.Max.Y; y++ {
		for x := area.Min.X; x < area.Max.X; x++ {
			img.Pix[img.PixOffset(x, y)+3] = 0
		}
	}
}

// hasTransparency reports whether some pixel in area is less than half opaque.
func hasTransparency(img *image.NRGBA, area image.Rectangle) bool {
	for y := area.Min.Y; y < area.Max.Y; y++ {
		for x := area.Min.X; x < area.Max.X; x++ {
			if img.Pix[img.PixOffset(x, y)+3] < 128 {
				return true
			}
		}
	}
	return false
}
