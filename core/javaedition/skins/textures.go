// Package skins reads the skins and capes Java Edition profiles name and prepares them as the
// 1.7.10 client draws them.
package skins

import (
	"crypto"
	"crypto/rsa"
	"crypto/sha1"
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/url"
	"strings"

	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
)

// texturesProperty names the profile property that carries a profile's textures.
const texturesProperty = "textures"

// textureDomains are the host suffixes a textures payload may name; a payload naming any
// other host names no textures.
var textureDomains = []string{".minecraft.net", ".mojang.com"}

// Textures are the addresses of the skin and cape a profile wears; an empty address means the
// default skin or no cape.
type Textures struct {
	Skin, Cape string
}

// errInsecure reports a textures property whose signature is missing or not the session
// service's.
var errInsecure = errors.New("java edition textures: the payload is not signed by the session service")

// decodeTextures reads the first textures property, as the 1.7.10 client does. With secure
// set the property must carry a signature that one of keys verifies; otherwise it names no
// textures and errInsecure is returned. A payload that does not decode, or that names a host
// outside textureDomains, names no textures.
func decodeTextures(properties []java.ProfileProperty, secure bool, keys []*rsa.PublicKey) (Textures, error) {
	var property *java.ProfileProperty
	for i := range properties {
		if properties[i].Name == texturesProperty {
			property = &properties[i]
			break
		}
	}
	if property == nil {
		return Textures{}, nil
	}
	if secure && !signed(property.Value, property.Signature, keys) {
		return Textures{}, errInsecure
	}
	data, err := decodeBase64(property.Value)
	if err != nil {
		return Textures{}, nil
	}
	var payload struct {
		Textures map[string]struct {
			URL string `json:"url"`
		} `json:"textures"`
	}
	if json.Unmarshal(data, &payload) != nil {
		return Textures{}, nil
	}
	for _, texture := range payload.Textures {
		if !textureDomain(texture.URL) {
			return Textures{}, nil
		}
	}
	return Textures{Skin: payload.Textures["SKIN"].URL, Cape: payload.Textures["CAPE"].URL}, nil
}

// signed reports whether signature, base64 text, is a SHA-1 RSA signature of value by one of
// keys.
func signed(value, signature string, keys []*rsa.PublicKey) bool {
	sig, err := decodeBase64(signature)
	if err != nil || len(sig) == 0 {
		return false
	}
	digest := sha1.Sum([]byte(value))
	for _, key := range keys {
		if rsa.VerifyPKCS1v15(key, crypto.SHA1, digest[:], sig) == nil {
			return true
		}
	}
	return false
}

// decodeBase64 reads base64 text with or without its padding.
func decodeBase64(text string) ([]byte, error) {
	return base64.RawStdEncoding.DecodeString(strings.TrimRight(text, "="))
}

// textureDomain reports whether address names a host in textureDomains.
func textureDomain(address string) bool {
	parsed, err := url.Parse(address)
	if err != nil {
		return false
	}
	host := parsed.Hostname()
	for _, domain := range textureDomains {
		if strings.HasSuffix(host, domain) {
			return true
		}
	}
	return false
}
