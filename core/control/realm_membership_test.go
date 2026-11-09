package control

import (
	"context"
	"errors"
	"net"
	"strings"
	"testing"
	"time"
)

// opaqueInviteCodes admits any code without a URL path.
type opaqueInviteCodes struct{}

func (opaqueInviteCodes) RealmInviteCode(input string) (string, error) {
	if strings.Contains(input, "/") {
		return "", errors.New("invitation link")
	}
	return input, nil
}

// membershipStub records whether resolving a code or accepting it was requested.
type membershipStub struct {
	stubServices
	opaqueInviteCodes
	accepted bool
	code     string
}

// RealmMembership returns an isolated invitation preview without an account service.
func (s *membershipStub) RealmMembership(_ context.Context, code string, accept bool) (Realm, error) {
	s.code, s.accepted = code, accept
	return Realm{Name: "Fixture Realm", Target: "realm_id/7"}, nil
}

func TestRealmMembershipSeparatesPreviewFromAcceptance(t *testing.T) {
	s := &membershipStub{}
	dir := startServices(t, NewStore(), s)
	for _, accept := range []bool{false, true} {
		params := `{"code":"fixture-code","accept":false}`
		if accept {
			params = `{"code":"fixture-code","accept":true}`
		}
		response := rpc(t, dir, "realm_membership.v1", params)
		if response.Error != nil {
			t.Fatalf("membership call failed: %+v", response.Error)
		}
		if s.code != "fixture-code" || s.accepted != accept {
			t.Fatalf("request: %q %v", s.code, s.accepted)
		}
	}
	s.code = ""
	if response := rpc(t, dir, "realm_membership.v1", `{"code":"a/b","accept":true}`); response.Error == nil ||
		response.Error.Code != codeInvalidTarget || s.code != "" {
		t.Fatalf("invalid code reached the service: %+v", response.Error)
	}
}

// blockingMembership observes cancellation propagated from a closed control stream.
type blockingMembership struct {
	stubServices
	opaqueInviteCodes
	started   chan struct{}
	cancelled chan struct{}
}

// RealmMembership waits until its request context is cancelled.
func (s *blockingMembership) RealmMembership(ctx context.Context, _ string, _ bool) (Realm, error) {
	close(s.started)
	<-ctx.Done()
	close(s.cancelled)
	return Realm{}, ctx.Err()
}

func TestRealmMembershipClosedStreamCancelsPreview(t *testing.T) {
	s := &blockingMembership{started: make(chan struct{}), cancelled: make(chan struct{})}
	serverConn, clientConn := net.Pipe()
	server := &Server{requestIOTimeout: time.Second}
	done := make(chan struct{})
	go func() {
		defer close(done)
		_, _ = server.serveRealmMembership(serverConn, []byte(`{"code":"fixture","accept":false}`), s)
		_ = serverConn.Close()
	}()
	<-s.started
	_ = clientConn.Close()
	<-s.cancelled
	<-done
}

// leaveStub records the Realm ids the leave service was asked to remove the account from.
type leaveStub struct {
	stubServices
	left []int
}

// LeaveRealm records the request without an account service.
func (s *leaveStub) LeaveRealm(_ context.Context, realmID int) error {
	s.left = append(s.left, realmID)
	return nil
}

// Only a positive Realm id reaches the leave service; anything else is rejected without a request.
func TestRealmLeaveValidatesTheRealmID(t *testing.T) {
	s := &leaveStub{}
	dir := startServices(t, NewStore(), s)
	if response := rpc(t, dir, "realm_leave.v1", `{"realm_id":7}`); response.Error != nil {
		t.Fatalf("leave failed: %+v", response.Error)
	}
	for _, params := range []string{`{"realm_id":0}`, `{"realm_id":-3}`} {
		if response := rpc(t, dir, "realm_leave.v1", params); response.Error == nil || response.Error.Code != codeInvalidTarget {
			t.Fatalf("%s: %+v", params, response.Error)
		}
	}
	for _, params := range []string{`{}`, `{"realm_id":"7"}`, `{"realm_id":7,"extra":1}`} {
		if response := rpc(t, dir, "realm_leave.v1", params); response.Error == nil {
			t.Fatalf("%s was accepted", params)
		}
	}
	if len(s.left) != 1 || s.left[0] != 7 {
		t.Fatalf("left = %v", s.left)
	}
}

// Services without Realm membership report the method as unavailable rather than failing silently.
func TestRealmLeaveWithoutMembershipServicesIsUnavailable(t *testing.T) {
	dir := startServices(t, NewStore(), &stubServices{})
	if response := rpc(t, dir, "realm_leave.v1", `{"realm_id":7}`); response.Error == nil || response.Error.Code != codeServicesDisabled {
		t.Fatalf("response: %+v", response.Error)
	}
}
