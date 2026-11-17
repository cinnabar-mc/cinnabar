package authcache

// EntitlementsPath returns where the last entitlement document for the token cache at oauthPath
// is kept.
func EntitlementsPath(oauthPath string) string {
	return oauthPath + ".entitlements"
}

// SaveEntitlements privately keeps the last signed entitlement document for the token cache at
// oauthPath. Nothing verifies its signature, so it is a record, not proof of ownership.
func SaveEntitlements(oauthPath string, signed []byte) error {
	if err := savePrivate(EntitlementsPath(oauthPath), signed); err != nil {
		return &cacheFailure{operation: "save_entitlements", err: err}
	}
	return nil
}
