package launcher

import (
	"context"
	"errors"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
)

func TestRealmMembershipNeedsAnAccountAndNeverSelectsAConnection(t *testing.T) {
	for _, signedIn := range []bool{false, true} {
		var account *authcache.Account
		if signedIn {
			account = testAccount()
			t.Cleanup(func() { _ = account.Close() })
		}
		f := newFixture(t, account)
		f.selector.Set("fixture.test:19132")
		calls := 0
		f.service.cfg.RealmMembership = func(_ context.Context, _ *authcache.Account, code string, accept bool) (catalog.Realm, error) {
			calls++
			if code != "fixture-code" || !accept {
				t.Fatalf("request: %q %v", code, accept)
			}
			return catalog.Realm{Name: "Fixture", Target: "realm_id/7"}, nil
		}
		_, err := f.service.RealmMembership(context.Background(), "https://realms.gg/fixture-code", true)
		if signedIn && err != nil || !signedIn && !errors.Is(err, control.ErrSignedOut) {
			t.Fatal(err)
		}
		if signedIn && calls != 1 || !signedIn && calls != 0 {
			t.Fatalf("calls=%d", calls)
		}
		if target, _ := f.selector.Target(); target != "fixture.test:19132" {
			t.Fatalf("selected %q", target)
		}
	}
}

func TestRealmMembershipDiscardsAccountRetiredWhileRequestRuns(t *testing.T) {
	account := testAccount()
	t.Cleanup(func() { _ = account.Close() })
	f := newFixture(t, account)
	f.service.cfg.RealmMembership = func(context.Context, *authcache.Account, string, bool) (catalog.Realm, error) {
		f.service.signedOut.Store(true)
		return catalog.Realm{Name: "Old account", Target: "realm_id/7"}, nil
	}
	if _, err := f.service.RealmMembership(context.Background(), "fixture", false); !errors.Is(err, control.ErrSignedOut) {
		t.Fatal(err)
	}
}

func TestLeaveRealmNeedsAnAccountAndAValidRealm(t *testing.T) {
	for _, signedIn := range []bool{false, true} {
		var account *authcache.Account
		if signedIn {
			account = testAccount()
			t.Cleanup(func() { _ = account.Close() })
		}
		f := newFixture(t, account)
		var left []int
		f.service.cfg.LeaveRealm = func(_ context.Context, _ *authcache.Account, realmID int) error {
			left = append(left, realmID)
			return nil
		}
		err := f.service.LeaveRealm(context.Background(), 7)
		if signedIn && err != nil || !signedIn && !errors.Is(err, control.ErrSignedOut) {
			t.Fatal(err)
		}
		if err := f.service.LeaveRealm(context.Background(), 0); err == nil {
			t.Fatal("an invalid Realm id was accepted")
		}
		if signedIn && len(left) != 1 || !signedIn && len(left) != 0 {
			t.Fatalf("left=%v", left)
		}
	}
}

func TestLeaveRealmReportsAccountRetiredWhileRequestRuns(t *testing.T) {
	account := testAccount()
	t.Cleanup(func() { _ = account.Close() })
	f := newFixture(t, account)
	f.service.cfg.LeaveRealm = func(context.Context, *authcache.Account, int) error {
		f.service.signedOut.Store(true)
		return nil
	}
	if err := f.service.LeaveRealm(context.Background(), 7); !errors.Is(err, control.ErrSignedOut) {
		t.Fatal(err)
	}
}
