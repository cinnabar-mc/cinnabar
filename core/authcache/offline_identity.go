package authcache

import (
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

// offlineIdentityNamespace versions the mapping from device and persona to login UUID.
// Changing it changes the identity of every offline profile.
var offlineIdentityNamespace = uuid.NewSHA1(uuid.NameSpaceURL, []byte("urn:cinnabar:offline-identity:v1"))

// OfflineIdentity returns an offline login identity scoped to a device and display name.
// A saved, nonzero device ID keeps it stable across core lifetimes. Missing or invalid
// device IDs get a fresh UUID, so callers without a profile never share a default identity.
// This identity must only be used for offline login; account tokens own online identity.
func OfflineIdentity(profile device.Profile, displayName string) login.IdentityData {
	var id uuid.UUID
	if material, ok := deviceIdentityBytes(profile.ID); ok {
		// Frame the variable-width device material separately from the persona.
		key := binary.BigEndian.AppendUint16(nil, uint16(len(material)))
		key = append(key, material...)
		key = append(key, displayName...)
		id = uuid.NewSHA1(offlineIdentityNamespace, key)
	} else {
		id = uuid.New()
	}
	return login.IdentityData{Identity: id.String(), DisplayName: displayName}
}

// deviceIdentityBytes canonicalizes supported device IDs and rejects zero placeholders.
func deviceIdentityBytes(id login.DeviceID) ([]byte, bool) {
	var material []byte
	switch id.Format() {
	case login.DeviceIDFormatLowerHexString, login.DeviceIDFormatUpperHexString:
		material, _ = hex.DecodeString(string(id))
	case login.DeviceIDFormatUUID:
		parsed, err := uuid.Parse(string(id))
		if err != nil {
			return nil, false
		}
		material = parsed[:]
	case login.DeviceIDFormatBase64:
		material, _ = base64.StdEncoding.DecodeString(string(id))
	default:
		return nil, false
	}
	for _, value := range material {
		if value != 0 {
			return material, true
		}
	}
	return nil, false
}
