package auth

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"golang.org/x/oauth2"
)

const notchID = "069a79f444e94726a5befca90e38aaf5"

func newTestAccounts(t *testing.T, app MicrosoftApp, services Services) *Accounts {
	t.Helper()
	accounts := OpenAccounts(AccountsConfig{Dir: t.TempDir(), App: app, Services: services})
	t.Cleanup(accounts.Close)
	return accounts
}

// saveAccounts writes an accounts file and a working token for each account.
func saveAccounts(t *testing.T, accounts *Accounts, selected string, saved ...Account) {
	t.Helper()
	data, err := json.Marshal(accountsIndex{Selected: selected, Accounts: saved})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(accounts.config.Dir, accountsIndexFile), data, 0o600); err != nil {
		t.Fatal(err)
	}
	token, err := json.Marshal(&oauth2.Token{AccessToken: "switch-access", RefreshToken: "switch-refresh", Expiry: time.Now().Add(time.Hour)})
	if err != nil {
		t.Fatal(err)
	}
	for _, account := range saved {
		if err := os.WriteFile(accounts.tokenPath(account.ID), token, 0o600); err != nil {
			t.Fatal(err)
		}
	}
}

func addIdle(accounts *Accounts) func() bool {
	return func() bool { return accounts.AddState().State == bridgecontract.JavaAccountAddIdle }
}

// Adding an account shows its code; once the player enters it the account is saved under
// its Java profile and selected, and the unfinished sign-in's token is gone.
func TestAccountsAddSavesAndSelectsTheProfile(t *testing.T) {
	fake := newFakeServices(t)
	app := newFakeApp()
	accounts := newTestAccounts(t, app, fake.services())
	prompt, err := accounts.BeginAdd(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if state := accounts.AddState(); state.State != bridgecontract.JavaAccountAddAwaitingCode || state.UserCode != prompt.UserCode {
		t.Fatalf("add state = %+v", state)
	}
	close(app.finish)
	testwait.Eventually(t, 5*time.Second, "the account to be added", addIdle(accounts))
	list, selected, err := accounts.List()
	if err != nil {
		t.Fatal(err)
	}
	if len(list) != 1 || list[0] != (Account{ID: notchID, Name: "Notch"}) || selected != notchID {
		t.Fatalf("accounts = %+v, selected %q", list, selected)
	}
	if _, err := os.Stat(accounts.tokenPath(notchID)); err != nil {
		t.Fatalf("the account's token was not saved: %v", err)
	}
	if _, err := os.Stat(filepath.Join(accounts.config.Dir, pendingTokenFile)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("the unfinished sign-in's token remains: %v", err)
	}
	account, keychain, ok := accounts.Selected()
	if !ok || account.Name != "Notch" {
		t.Fatalf("selected = %+v, %v", account, ok)
	}
	if session, err := keychain.Session(context.Background()); err != nil || session.Profile.Name != "Notch" {
		t.Fatalf("selected session = %+v, %v", session, err)
	}
}

// A Microsoft account without Java Edition is not added, and the reason says so.
func TestAccountsAddWithoutJavaEditionFails(t *testing.T) {
	fake := newFakeServices(t)
	fake.profileStatus = http.StatusNotFound
	app := newFakeApp()
	accounts := newTestAccounts(t, app, fake.services())
	if _, err := accounts.BeginAdd(context.Background()); err != nil {
		t.Fatal(err)
	}
	close(app.finish)
	failed := func() bool { return accounts.AddState().State == bridgecontract.JavaAccountAddFailed }
	testwait.Eventually(t, 5*time.Second, "the add to fail", failed)
	if reason := accounts.AddState().Reason; reason != "This Microsoft account does not own Java Edition." {
		t.Fatalf("reason = %q", reason)
	}
	if list, _, _ := accounts.List(); len(list) != 0 {
		t.Fatalf("accounts = %+v", list)
	}
}

// Cancelling an add stops it: entering the code afterwards adds nothing.
func TestAccountsCancelStopsTheAdd(t *testing.T) {
	fake := newFakeServices(t)
	app := newFakeApp()
	accounts := newTestAccounts(t, app, fake.services())
	if _, err := accounts.BeginAdd(context.Background()); err != nil {
		t.Fatal(err)
	}
	accounts.CancelAdd()
	close(app.finish)
	accounts.work.Wait()
	if state := accounts.AddState(); state.State != bridgecontract.JavaAccountAddIdle {
		t.Fatalf("add state = %+v", state)
	}
	if list, _, _ := accounts.List(); len(list) != 0 {
		t.Fatalf("a cancelled add saved %+v", list)
	}
}

// Selecting changes the account joins use; removing the selected one selects another and
// deletes its token.
func TestAccountsSelectAndRemove(t *testing.T) {
	accounts := newTestAccounts(t, newFakeApp(), newFakeServices(t).services())
	alex := Account{ID: "ec561538f3fd461daff5086b22154bce", Name: "Alex"}
	steve := Account{ID: "8667ba71b85a4004af54457a9734eed7", Name: "Steve"}
	saveAccounts(t, accounts, alex.ID, alex, steve)

	if err := accounts.Select(steve.ID); err != nil {
		t.Fatal(err)
	}
	if account, _, ok := accounts.Selected(); !ok || account != steve {
		t.Fatalf("selected %+v after selecting Steve", account)
	}
	if err := accounts.Select("ffffffffffffffffffffffffffffffff"); !errors.Is(err, ErrNoAccount) {
		t.Fatalf("selecting an unknown account = %v", err)
	}
	if err := accounts.Remove(context.Background(), steve.ID); err != nil {
		t.Fatal(err)
	}
	if account, _, ok := accounts.Selected(); !ok || account != alex {
		t.Fatalf("selected %+v after removing Steve", account)
	}
	if _, err := os.Stat(accounts.tokenPath(steve.ID)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("Steve's token remains: %v", err)
	}
	if err := accounts.Rename(alex.ID, "Alexandra"); err != nil {
		t.Fatal(err)
	}
	if list, _, _ := accounts.List(); len(list) != 1 || list[0].Name != "Alexandra" {
		t.Fatalf("accounts after the rename = %+v", list)
	}
}

// A code that expires or is declined ends the add with a reason instead of leaving it
// waiting, and a new add can start.
func TestAccountsADeclinedCodeFailsTheAdd(t *testing.T) {
	app := newFakeApp()
	app.declined = true
	accounts := newTestAccounts(t, app, newFakeServices(t).services())
	if _, err := accounts.BeginAdd(context.Background()); err != nil {
		t.Fatal(err)
	}
	close(app.finish)
	failed := func() bool { return accounts.AddState().State == bridgecontract.JavaAccountAddFailed }
	testwait.Eventually(t, 5*time.Second, "the add to fail", failed)
	if reason := accounts.AddState().Reason; reason != addCodeFailed {
		t.Fatalf("reason = %q", reason)
	}
	if _, err := accounts.BeginAdd(context.Background()); err != nil {
		t.Fatalf("a new add after the failure = %v", err)
	}
	if started, _ := app.counts(); started != 2 {
		t.Fatalf("%d device codes requested", started)
	}
}

// While the entered code's profile is being read, another Add hands out no code that would
// never be accepted, and Cancel stops the account from being added.
func TestAccountsCancelWhileReadingTheProfile(t *testing.T) {
	app := newFakeApp()
	app.stall = make(chan struct{})
	accounts := newTestAccounts(t, app, newFakeServices(t).services())
	if _, err := accounts.BeginAdd(context.Background()); err != nil {
		t.Fatal(err)
	}
	close(app.finish)
	signingIn := func() bool { return accounts.AddState().State == bridgecontract.JavaAccountAddSigningIn }
	testwait.Eventually(t, 5*time.Second, "the profile to be read", signingIn)
	if _, err := accounts.BeginAdd(context.Background()); !errors.Is(err, ErrSignInUnavailable) {
		t.Fatalf("an add while reading the profile = %v", err)
	}
	if started, _ := app.counts(); started != 1 {
		t.Fatalf("%d device codes requested", started)
	}
	accounts.CancelAdd()
	close(app.stall)
	accounts.work.Wait()
	if list, _, _ := accounts.List(); len(list) != 0 {
		t.Fatalf("a cancelled add saved %+v", list)
	}
	if state := accounts.AddState(); state.State != bridgecontract.JavaAccountAddIdle {
		t.Fatalf("add state = %+v", state)
	}
}

// An unreadable accounts file is kept aside and the accounts start over, so the player can
// add an account again.
func TestAccountsAnUnreadableIndexStartsOver(t *testing.T) {
	accounts := newTestAccounts(t, newFakeApp(), newFakeServices(t).services())
	path := filepath.Join(accounts.config.Dir, accountsIndexFile)
	if err := os.WriteFile(path, []byte("{not json"), 0o600); err != nil {
		t.Fatal(err)
	}
	list, selected, err := accounts.List()
	if err != nil || len(list) != 0 || selected != "" {
		t.Fatalf("list = %+v, %q, %v", list, selected, err)
	}
	if _, err := os.Stat(path + ".corrupt"); err != nil {
		t.Fatalf("the unreadable file was not kept aside: %v", err)
	}
}
