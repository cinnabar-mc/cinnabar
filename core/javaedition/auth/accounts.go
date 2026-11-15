package auth

import (
	"context"
	"crypto/rand"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
)

// Files of an accounts directory: the index of accounts and its lock. Each account's token is
// "<id>.json", and each unfinished sign-in's token "pending-<random>.json", so cores sharing
// the directory never share one.
const (
	accountsIndexFile  = "accounts.json"
	accountsLockFile   = "accounts.lock"
	pendingTokenPrefix = "pending-"
)

// stalePendingAge is how old an unfinished sign-in's files are once no add can still use
// them: past a device code's lifetime and the profile read after it, as after a core crashed.
const stalePendingAge = time.Hour

// errAddCancelled reports an add cancelled before its account was filed.
var errAddCancelled = errors.New("the Java Edition account add was cancelled")

// indexLockTimeout bounds the wait for another process changing the accounts file.
const indexLockTimeout = 10 * time.Second

// profileReadTimeout bounds reading a new account's Java profile once its code is entered.
const profileReadTimeout = time.Minute

// profileIDPattern is a Java profile id as the services return it: 32 hexadecimal digits.
var profileIDPattern = regexp.MustCompile(`^[0-9a-f]{32}$`)

// ErrAddInProgress reports an add whose code was entered and whose profile is being read.
var ErrAddInProgress = errors.New("a Java Edition account is being added")

// ErrNoAccount reports an account id that is not saved.
var ErrNoAccount = errors.New("no such Java Edition account")

// Account is a saved Java Edition account: its Java profile.
type Account struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

// AddState is the progress of adding an account. State is one of the JavaAccountAdd values
// of the bridge contract; the code fields are set while it waits for the player, and Reason
// once it failed.
type AddState struct {
	State           string
	VerificationURI string
	UserCode        string
	Reason          string
}

// accountsIndex is the accounts file: every account and the selected one.
type accountsIndex struct {
	Selected string    `json:"selected,omitempty"`
	Accounts []Account `json:"accounts"`
}

// AccountsConfig configures Accounts.
type AccountsConfig struct {
	// Dir holds the accounts' tokens and index.
	Dir string
	// App, Services, Logger and Now configure each account's Keychain.
	App      MicrosoftApp
	Services Services
	Logger   *slog.Logger
	Now      func() time.Time
}

// Accounts is the install's Java Edition accounts, independent of its Bedrock accounts. Each
// is a sign-in through an app the Java Edition services accept, saved with its Java profile;
// the selected one signs in Java joins. Several processes may share a directory: the index is
// replaced atomically and reread on every call.
type Accounts struct {
	config AccountsConfig
	// lifetime ends with Close, stopping the head pictures being read.
	lifetime context.Context
	stop     context.CancelFunc
	work     sync.WaitGroup

	mu sync.Mutex
	// selected is the open keychain of the selected account, and selectedID its id.
	selected   *Keychain
	selectedID string
	// adding is the keychain of the sign-in under way, if any.
	adding *Keychain
	add    AddState
	closed bool
	// heads holds each account's head picture reads.
	heads map[string]headFetch
	// writeIndex replaces the accounts file; tests make it fail.
	writeIndex func(path string, data []byte) error
}

// OpenAccounts returns the accounts kept in config.Dir. Close stops their background work.
func OpenAccounts(config AccountsConfig) *Accounts {
	if config.Logger == nil {
		config.Logger = slog.New(slog.DiscardHandler)
	}
	if config.Now == nil {
		config.Now = time.Now
	}
	lifetime, stop := context.WithCancel(context.Background())
	return &Accounts{
		config: config, lifetime: lifetime, stop: stop,
		add: AddState{State: bridgecontract.JavaAccountAddIdle}, heads: map[string]headFetch{},
		writeIndex: writeFileAtomic,
	}
}

// Close stops the sign-ins under way and closes the open keychains.
func (a *Accounts) Close() {
	a.mu.Lock()
	a.closed = true
	selected, adding := a.selected, a.adding
	a.selected, a.adding = nil, nil
	a.mu.Unlock()
	if selected != nil {
		selected.Close()
	}
	if adding != nil {
		adding.Close()
	}
	a.stop()
	a.work.Wait()
}

// List returns the saved accounts, by name, and the selected one's id.
func (a *Accounts) List() ([]Account, string, error) {
	index, err := a.readIndex()
	if err != nil {
		return nil, "", err
	}
	return index.Accounts, index.Selected, nil
}

// Selected returns the selected account and its keychain, or false when none is selected.
func (a *Accounts) Selected() (Account, *Keychain, bool) {
	index, err := a.readIndex()
	if err != nil {
		a.config.Logger.Warn("Java Edition accounts unreadable", "error", err)
		return Account{}, nil, false
	}
	account, ok := index.find(index.Selected)
	if !ok {
		return Account{}, nil, false
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.closed {
		return Account{}, nil, false
	}
	if a.selected == nil || a.selectedID != account.ID {
		if previous := a.selected; previous != nil {
			// Closed in the background, which Close waits for.
			a.work.Add(1)
			go func() {
				defer a.work.Done()
				previous.Close()
			}()
		}
		a.selected, a.selectedID = a.keychain(a.tokenPath(account.ID), nil), account.ID
	}
	return account, a.selected, true
}

// Select makes the account with id the one Java joins use.
func (a *Accounts) Select(id string) error {
	return a.updateIndex(func(index *accountsIndex) error {
		if _, ok := index.find(id); !ok {
			return ErrNoAccount
		}
		index.Selected = id
		return nil
	})
}

// Remove deletes the account with id and its sign-in. Removing the selected account selects
// the first remaining one.
func (a *Accounts) Remove(ctx context.Context, id string) error {
	err := a.updateIndex(func(index *accountsIndex) error {
		if _, ok := index.find(id); !ok {
			return ErrNoAccount
		}
		kept := index.Accounts[:0]
		for _, account := range index.Accounts {
			if account.ID != id {
				kept = append(kept, account)
			}
		}
		index.Accounts = kept
		if index.Selected == id {
			index.Selected = ""
			if len(kept) > 0 {
				index.Selected = kept[0].ID
			}
		}
		return nil
	})
	if err != nil {
		return err
	}
	a.mu.Lock()
	var closing *Keychain
	if a.selectedID == id {
		closing, a.selected, a.selectedID = a.selected, nil, ""
	}
	a.mu.Unlock()
	if closing != nil {
		closing.Close()
	}
	if err := os.Remove(a.headPath(id)); err != nil && !errors.Is(err, os.ErrNotExist) {
		a.config.Logger.Warn("Java Edition account picture not removed", "error", err)
	}
	return authcache.Remove(ctx, a.tokenPath(id), os.Remove)
}

// Rename records a new profile name for the account with id, as after the player renamed
// the profile.
func (a *Accounts) Rename(id, name string) error {
	if name == "" {
		return errors.New("java edition profile name is empty")
	}
	return a.updateIndex(func(index *accountsIndex) error {
		i := index.position(id)
		if i < 0 {
			return ErrNoAccount
		}
		index.Accounts[i].Name = name
		return nil
	})
}

// AddState reports the progress of adding an account.
func (a *Accounts) AddState() AddState {
	a.mu.Lock()
	defer a.mu.Unlock()
	return a.add
}

// BeginAdd starts the device-code sign-in of a new account, or returns the one already
// waiting for the player. Once the player enters the code, the account is saved with its
// Java profile and selected; signing in an account already saved replaces its token.
func (a *Accounts) BeginAdd(ctx context.Context) (*SignInRequired, error) {
	a.mu.Lock()
	if a.closed {
		a.mu.Unlock()
		return nil, fmt.Errorf("%w: the accounts are closed", ErrSignInUnavailable)
	}
	if a.add.State == bridgecontract.JavaAccountAddSigningIn {
		// The code was entered; a new one would never be accepted.
		a.mu.Unlock()
		return nil, ErrAddInProgress
	}
	adding := a.adding
	if adding == nil {
		if err := os.MkdirAll(a.config.Dir, 0o700); err != nil {
			a.add = AddState{State: bridgecontract.JavaAccountAddFailed, Reason: addStartFailed}
			a.mu.Unlock()
			return nil, fmt.Errorf("%w: %v", ErrSignInUnavailable, err)
		}
		a.removeStalePending()
		var keychain *Keychain
		keychain = NewKeychain(KeychainConfig{
			Path: filepath.Join(a.config.Dir, pendingTokenPrefix+rand.Text()+".json"), App: a.config.App,
			Services: a.config.Services, Logger: a.config.Logger, Now: a.config.Now,
			OnSignedIn:     func() { a.finishAdd(keychain) },
			OnSignInFailed: func(error) { a.failAdd(keychain) },
		})
		a.adding, adding = keychain, keychain
	}
	a.mu.Unlock()
	prompt, err := adding.BeginSignIn(ctx)
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.adding != adding {
		// Cancelled or closed meanwhile.
		if err == nil {
			err = fmt.Errorf("%w: the sign-in was cancelled", ErrSignInUnavailable)
		}
		return nil, err
	}
	if err != nil {
		a.add = AddState{State: bridgecontract.JavaAccountAddFailed, Reason: addStartFailed}
		return nil, err
	}
	a.add = AddState{State: bridgecontract.JavaAccountAddAwaitingCode, VerificationURI: prompt.VerificationURI, UserCode: prompt.UserCode}
	return prompt, nil
}

// Reasons an add failed, as the Accounts dialog shows them.
const (
	addStartFailed   = "The Java sign-in could not be started. Try again."
	addCodeFailed    = "The sign-in did not finish: the code expired, it was declined, or it could not be saved. Try again."
	addFilingFailed  = "The Java account could not be added. Try again."
	addNoJavaEdition = "This Microsoft account does not own Java Edition."
)

// CancelAdd stops the sign-in under way, even while its profile is being read, and forgets
// its token.
func (a *Accounts) CancelAdd() {
	a.mu.Lock()
	adding := a.adding
	a.adding = nil
	a.add = AddState{State: bridgecontract.JavaAccountAddIdle}
	a.mu.Unlock()
	if adding != nil {
		a.discard(adding)
	}
}

// failAdd records a sign-in that ended without a token, such as an expired code.
func (a *Accounts) failAdd(adding *Keychain) {
	a.mu.Lock()
	if a.adding != adding {
		a.mu.Unlock()
		return
	}
	a.adding = nil
	a.add = AddState{State: bridgecontract.JavaAccountAddFailed, Reason: addCodeFailed}
	a.mu.Unlock()
	a.discard(adding)
}

// discard closes an unfinished sign-in's keychain and deletes its token.
func (a *Accounts) discard(adding *Keychain) {
	adding.Close()
	ctx, cancel := context.WithTimeout(context.Background(), profileReadTimeout)
	defer cancel()
	_ = adding.Forget(ctx)
}

// finishAdd reads the Java profile of a finished sign-in, files its token under the profile's
// id and selects the account, unless the add is cancelled first.
func (a *Accounts) finishAdd(adding *Keychain) {
	a.mu.Lock()
	if a.adding != adding || a.closed {
		a.mu.Unlock()
		return
	}
	a.add = AddState{State: bridgecontract.JavaAccountAddSigningIn}
	a.work.Add(1)
	a.mu.Unlock()
	defer a.work.Done()

	ctx, cancel := context.WithTimeout(context.Background(), profileReadTimeout)
	session, err := adding.Session(ctx)
	cancel()
	if err == nil && !profileIDPattern.MatchString(session.Profile.ID) {
		err = fmt.Errorf("java edition profile id %q is malformed", session.Profile.ID)
	}
	a.mu.Lock()
	cancelled := a.adding != adding
	a.mu.Unlock()
	adding.Close()
	if cancelled {
		// CancelAdd already discards the token.
		return
	}
	if err == nil {
		err = a.file(adding, session.Profile)
	}
	a.discard(adding)
	if errors.Is(err, errAddCancelled) {
		// CancelAdd already reset the add's state.
		return
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.adding != adding {
		return
	}
	a.adding = nil
	if err != nil {
		a.config.Logger.Warn("Java Edition account could not be added", "error", err)
		reason := addFilingFailed
		if errors.Is(err, ErrNoProfile) {
			reason = addNoJavaEdition
		}
		a.add = AddState{State: bridgecontract.JavaAccountAddFailed, Reason: reason}
		return
	}
	a.add = AddState{State: bridgecontract.JavaAccountAddIdle}
	a.config.Logger.Info("Java Edition account added")
}

// file moves the finished sign-in's token to the account's own file and records the account
// as the selected one, unless the add was cancelled first. It returns errAddCancelled then.
func (a *Accounts) file(adding *Keychain, profile Profile) error {
	target := a.tokenPath(profile.ID)
	a.mu.Lock()
	var closing *Keychain
	if a.selectedID == profile.ID {
		closing, a.selected, a.selectedID = a.selected, nil, ""
	}
	a.mu.Unlock()
	if closing != nil {
		closing.Close()
	}
	var moved, replaced bool
	err := a.updateIndex(func(index *accountsIndex) error {
		// Checked under the accounts lock, so a cancel either comes first and files nothing
		// or comes after the account was added.
		a.mu.Lock()
		cancelled := a.adding != adding
		a.mu.Unlock()
		if cancelled {
			return errAddCancelled
		}
		ctx, cancel := context.WithTimeout(context.Background(), profileReadTimeout)
		defer cancel()
		// Both tokens' leases keep a refresh of either, in any core, out of the move; a rename
		// keeps the private permissions the token cache gave the file.
		_, statErr := os.Stat(target)
		replaced = statErr == nil
		if err := authcache.Move(ctx, adding.config.Path, target); err != nil {
			return err
		}
		moved = true
		account := Account{ID: profile.ID, Name: profile.Name}
		if i := index.position(profile.ID); i >= 0 {
			index.Accounts[i] = account
		} else {
			index.Accounts = append(index.Accounts, account)
		}
		index.Selected = profile.ID
		return nil
	})
	if err != nil && moved && !replaced {
		// The accounts file did not take the new account, so its token would sit there
		// unlisted. A replaced token stays: its account is still listed and signs in with it.
		ctx, cancel := context.WithTimeout(context.Background(), profileReadTimeout)
		defer cancel()
		if removeErr := authcache.Remove(ctx, target, os.Remove); removeErr != nil {
			err = errors.Join(err, removeErr)
		}
	}
	return err
}

// removeStalePending deletes unfinished sign-ins' files that no add can still use, such as
// those a crashed core left.
func (a *Accounts) removeStalePending() {
	entries, err := os.ReadDir(a.config.Dir)
	if err != nil {
		return
	}
	for _, entry := range entries {
		// "pending.json" is the single unfinished sign-in file of earlier builds.
		if !strings.HasPrefix(entry.Name(), pendingTokenPrefix) && !strings.HasPrefix(entry.Name(), "pending.json") {
			continue
		}
		info, err := entry.Info()
		if err != nil || a.config.Now().Sub(info.ModTime()) < stalePendingAge {
			continue
		}
		_ = os.Remove(filepath.Join(a.config.Dir, entry.Name()))
	}
}

// keychain opens the keychain of the token at path.
func (a *Accounts) keychain(path string, onSignedIn func()) *Keychain {
	return NewKeychain(KeychainConfig{
		Path: path, App: a.config.App, Services: a.config.Services,
		Logger: a.config.Logger, Now: a.config.Now, OnSignedIn: onSignedIn,
	})
}

func (a *Accounts) tokenPath(id string) string {
	return filepath.Join(a.config.Dir, id+".json")
}

// readIndex reads the accounts file; a missing one has no accounts.
func (a *Accounts) readIndex() (accountsIndex, error) {
	data, err := os.ReadFile(filepath.Join(a.config.Dir, accountsIndexFile))
	if errors.Is(err, os.ErrNotExist) {
		return accountsIndex{}, nil
	}
	if err != nil {
		return accountsIndex{}, err
	}
	var index accountsIndex
	if err := json.Unmarshal(data, &index); err != nil {
		// An unreadable file would lock the player out of every account, so it reads as no
		// accounts; the next change keeps it aside under the lock.
		return accountsIndex{}, nil
	}
	valid := index.Accounts[:0]
	for _, account := range index.Accounts {
		if profileIDPattern.MatchString(account.ID) && account.Name != "" {
			valid = append(valid, account)
		}
	}
	index.Accounts = valid
	if _, ok := index.find(index.Selected); !ok {
		index.Selected = ""
	}
	return index, nil
}

// updateIndex applies change to the accounts file under the directory's lock.
func (a *Accounts) updateIndex(change func(*accountsIndex) error) error {
	lease, err := a.lockIndex()
	if err != nil {
		return err
	}
	defer func() { _ = lease.Close() }()
	a.keepUnreadableIndex()
	index, err := a.readIndex()
	if err != nil {
		return err
	}
	if err := change(&index); err != nil {
		return err
	}
	data, err := json.Marshal(index)
	if err != nil {
		return err
	}
	return a.writeIndex(filepath.Join(a.config.Dir, accountsIndexFile), data)
}

// lockIndex takes the accounts directory's lock, which every change to the accounts file and
// every write that depends on an account still being listed holds.
func (a *Accounts) lockIndex() (io.Closer, error) {
	if err := os.MkdirAll(a.config.Dir, 0o700); err != nil {
		return nil, err
	}
	return lockfile.Acquire(filepath.Join(a.config.Dir, accountsLockFile), indexLockTimeout)
}

// keepUnreadableIndex moves an accounts file that does not parse aside, so the change about
// to be written does not discard it unseen. The caller holds the directory's lock.
func (a *Accounts) keepUnreadableIndex() {
	path := filepath.Join(a.config.Dir, accountsIndexFile)
	data, err := os.ReadFile(path)
	if err != nil || json.Valid(data) {
		return
	}
	a.config.Logger.Warn("Java Edition accounts file unreadable; keeping it aside and starting over")
	_ = os.Rename(path, path+".corrupt")
}

func (index accountsIndex) find(id string) (Account, bool) {
	if i := index.position(id); i >= 0 {
		return index.Accounts[i], true
	}
	return Account{}, false
}

func (index accountsIndex) position(id string) int {
	for i, account := range index.Accounts {
		if id != "" && account.ID == id {
			return i
		}
	}
	return -1
}

// writeFileAtomic replaces path with data, readable by the owner only.
func writeFileAtomic(path string, data []byte) error {
	temporary, err := os.CreateTemp(filepath.Dir(path), filepath.Base(path)+".*.tmp")
	if err != nil {
		return err
	}
	name := temporary.Name()
	_, writeErr := temporary.Write(data)
	closeErr := temporary.Close()
	if err := errors.Join(writeErr, closeErr); err != nil {
		_ = os.Remove(name)
		return err
	}
	if err := os.Chmod(name, 0o600); err != nil {
		_ = os.Remove(name)
		return err
	}
	if err := os.Rename(name, path); err != nil {
		_ = os.Remove(name)
		return err
	}
	return nil
}
