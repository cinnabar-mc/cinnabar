package authflow

import (
	"context"
	"errors"
	"fmt"

	"github.com/df-mc/go-xsapi/v2/xal"
	"github.com/sandertv/gophertunnel/minecraft/auth/mcservices"
	"golang.org/x/oauth2"
)

// minecraftServices checks ownership through gophertunnel's Minecraft services client.
type minecraftServices struct {
	config *oauth2.Config
}

// MinecraftServices returns the license check for Cinnabar's app registration clientID.
func MinecraftServices(clientID string) Licenses {
	return minecraftServices{config: mcservices.OAuthConfig(clientID)}
}

func (m minecraftServices) DeviceAuth(ctx context.Context) (*oauth2.DeviceAuthResponse, error) {
	return m.config.DeviceAuth(ctx)
}

func (m minecraftServices) DeviceToken(ctx context.Context, response *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
	return m.config.DeviceAccessToken(ctx, response)
}

func (m minecraftServices) TokenSource(ctx context.Context, token *oauth2.Token) oauth2.TokenSource {
	return m.config.TokenSource(ctx, token)
}

func (m minecraftServices) Check(ctx context.Context, token *oauth2.Token) LicenseCheck {
	entitlements, err := mcservices.Fetch(ctx, nil, token)
	return classifyEntitlements(entitlements, err)
}

// classifyEntitlements reduces a Fetch answer to a result. Only a successful answer without a
// Minecraft item means not owned; every error says nothing about ownership.
func classifyEntitlements(entitlements *mcservices.Entitlements, err error) LicenseCheck {
	var xbox *mcservices.XboxError
	var status *xal.StatusError
	switch {
	case err == nil && entitlements.PlaysMinecraft():
		return LicenseCheck{Result: ResultVerified, Reason: "entitled", Signed: entitlements.Signed}
	case err == nil:
		return LicenseCheck{Result: ResultNotOwned, Reason: "no_minecraft_entitlement", Signed: entitlements.Signed}
	case errors.Is(err, mcservices.ErrUnauthorized):
		return LicenseCheck{Result: ResultSignInRequired, Reason: "unauthorized"}
	case errors.Is(err, mcservices.ErrAppNotApproved):
		return LicenseCheck{Result: ResultUnverifiable, Reason: "app_not_approved"}
	case errors.Is(err, mcservices.ErrMalformedResponse):
		return LicenseCheck{Result: ResultUnverifiable, Reason: "malformed_response"}
	case errors.Is(err, mcservices.ErrRequestIDMismatch):
		return LicenseCheck{Result: ResultUnverifiable, Reason: "request_id_mismatch"}
	case errors.As(err, &xbox):
		return LicenseCheck{Result: ResultUnverifiable, Reason: fmt.Sprintf("xerr_%d", xbox.XErr)}
	case errors.As(err, &status):
		return LicenseCheck{Result: ResultUnverifiable, Reason: fmt.Sprintf("http_%d", status.StatusCode)}
	default:
		return LicenseCheck{Result: ResultUnverifiable, Reason: failureReason(err)}
	}
}
