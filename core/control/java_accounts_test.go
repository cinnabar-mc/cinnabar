package control

import (
	"context"
	"fmt"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

// fakeJavaAccounts records the actions it was asked for.
type fakeJavaAccounts struct {
	accounts  []JavaAccount
	selected  string
	add       JavaAccountAdd
	cancelled bool
}

func (f *fakeJavaAccounts) List() ([]JavaAccount, string, error) { return f.accounts, f.selected, nil }
func (f *fakeJavaAccounts) Add() JavaAccountAdd                  { return f.add }
func (f *fakeJavaAccounts) CancelAdd()                           { f.cancelled = true }

func (f *fakeJavaAccounts) BeginAdd(context.Context) error {
	f.add = JavaAccountAdd{State: bridgecontract.JavaAccountAddAwaitingCode, VerificationURI: "https://www.microsoft.com/link", UserCode: "TESTCODE"}
	return nil
}

func (f *fakeJavaAccounts) Select(id string) error {
	for _, account := range f.accounts {
		if account.ID == id {
			f.selected = id
			return nil
		}
	}
	return ErrNoJavaAccount
}

func (f *fakeJavaAccounts) Remove(_ context.Context, id string) error {
	for i, account := range f.accounts {
		if account.ID == id {
			f.accounts = append(f.accounts[:i], f.accounts[i+1:]...)
			return nil
		}
	}
	return ErrNoJavaAccount
}

// Every action answers with the accounts, the selected one and the sign-in under way.
func TestJavaAccountsControl(t *testing.T) {
	dir := t.TempDir()
	server, err := Start(dir, NewStore())
	if err != nil {
		t.Fatal(err)
	}
	defer server.Close()
	request := func(params string) string {
		return string(exchange(t, dir, []byte(fmt.Sprintf(`{"jsonrpc":"2.0","id":1,"method":"java_accounts.v1","params":%s}`, params))))
	}
	assertRPCError(t, []byte(request(`{"action":"list"}`)), -32601)
	accounts := &fakeJavaAccounts{accounts: []JavaAccount{{ID: "a", Name: "Alex"}, {ID: "s", Name: "Steve"}}, selected: "a"}
	server.SetJavaAccounts(accounts)

	for _, invalid := range []string{`{}`, `{"action":"fly"}`, `{"action":"list","extra":1}`, `{"action":"select","id":"nobody"}`, `{"action":"remove","id":"nobody"}`} {
		assertRPCError(t, []byte(request(invalid)), -32602)
	}
	if listed := request(`{"action":"list"}`); !strings.Contains(listed, `"accounts":[{"id":"a","name":"Alex"},{"id":"s","name":"Steve"}]`) || !strings.Contains(listed, `"selected":"a"`) {
		t.Fatalf("list = %s", listed)
	}
	if added := request(`{"action":"add"}`); !strings.Contains(added, `"user_code":"TESTCODE"`) || !strings.Contains(added, `"state":"awaiting_code"`) {
		t.Fatalf("add = %s", added)
	}
	if request(`{"action":"cancel"}`); !accounts.cancelled {
		t.Fatal("cancel did not reach the accounts")
	}
	if selected := request(`{"action":"select","id":"s"}`); !strings.Contains(selected, `"selected":"s"`) {
		t.Fatalf("select = %s", selected)
	}
	if removed := request(`{"action":"remove","id":"a"}`); strings.Contains(removed, `"Alex"`) {
		t.Fatalf("remove = %s", removed)
	}
}
