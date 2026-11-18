package control

import (
	"context"
	"encoding/json"
	"errors"
	"net"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

const (
	methodRealmMembership = bridgecontract.MethodRealmMembership
	methodRealmLeave      = bridgecontract.MethodRealmLeave
)

var errMembershipUnavailable = errors.New("control: Realm membership unavailable")

// RealmMembershipServices separates invitation preview from explicit membership acceptance.
type RealmMembershipServices interface {
	// RealmInviteCode extracts the opaque code from an invitation link or code.
	RealmInviteCode(input string) (string, error)
	RealmMembership(ctx context.Context, code string, accept bool) (Realm, error)
}

// RealmLeaveServices removes the signed-in account from a Realm it joined.
type RealmLeaveServices interface {
	LeaveRealm(ctx context.Context, realmID int) error
}

// serveRealmLeave validates the Realm id before any service request. Leaving is not cancelled
// when the caller disconnects, because the service may already have applied it.
func serveRealmLeave(ctx context.Context, raw json.RawMessage, services Services) (any, error) {
	var params struct {
		RealmID *int `json:"realm_id"`
	}
	if !decodeParams(raw, &params) || params.RealmID == nil {
		return nil, errInvalidParams
	}
	leave, ok := services.(RealmLeaveServices)
	if !ok {
		return nil, errMembershipUnavailable
	}
	if *params.RealmID <= 0 {
		return nil, ErrInvalidTarget
	}
	if err := leave.LeaveRealm(ctx, *params.RealmID); err != nil {
		return nil, err
	}
	return emptyResultV1{SchemaVersion: bridgecontract.ControlSchemaVersion}, nil
}

// serveRealmMembership cancels the request when its one-shot control connection closes.
func (server *Server) serveRealmMembership(conn net.Conn, raw json.RawMessage, services Services) (any, error) {
	var params struct {
		Code   *string `json:"code"`
		Accept *bool   `json:"accept"`
	}
	if !decodeParams(raw, &params) || params.Code == nil || params.Accept == nil {
		return nil, errInvalidParams
	}
	membership, ok := services.(RealmMembershipServices)
	if !ok {
		return nil, errMembershipUnavailable
	}
	code, err := membership.RealmInviteCode(*params.Code)
	if err != nil {
		return nil, ErrInvalidTarget
	}
	ctx, cancel := context.WithTimeout(context.Background(), serviceCallTimeout)
	defer cancel()
	if err := conn.SetReadDeadline(time.Time{}); err != nil {
		return nil, err
	}
	finished := make(chan struct{})
	go func() {
		defer close(finished)
		var extra [1]byte
		_, _ = conn.Read(extra[:])
		cancel()
	}()
	defer func() { _ = conn.SetReadDeadline(time.Now()); <-finished }()
	realm, err := membership.RealmMembership(ctx, code, *params.Accept)
	if err != nil {
		return nil, err
	}
	return struct {
		SchemaVersion uint32 `json:"schema_version"`
		Code          string `json:"code"`
		Realm         Realm  `json:"realm"`
	}{SchemaVersion: bridgecontract.ControlSchemaVersion, Code: code, Realm: realm}, nil
}
