package control

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

const methodJavaAccounts = bridgecontract.MethodJavaAccounts

// javaAccountsTimeout bounds one java_accounts.v1 request; adding an account only starts its
// sign-in, which then runs in the background.
const javaAccountsTimeout = 30 * time.Second

// ErrNoJavaAccount reports an action naming an account that is not saved.
var ErrNoJavaAccount = errors.New("no such Java Edition account")

// JavaAccount is a saved Java Edition account: its Java profile id and name.
type JavaAccount struct {
	ID   string `json:"id"`
	Name string `json:"name"`
	// Picture is the path of the account's head picture, when it has one.
	Picture string `json:"picture,omitempty"`
}

// JavaAccountAdd is the progress of adding a Java account. State is a JavaAccountAdd value
// of the bridge contract.
type JavaAccountAdd struct {
	State           string `json:"state"`
	VerificationURI string `json:"verification_uri,omitempty"`
	UserCode        string `json:"user_code,omitempty"`
	Reason          string `json:"reason,omitempty"`
}

// JavaAccounts is the install's Java Edition accounts behind java_accounts.v1.
type JavaAccounts interface {
	List() ([]JavaAccount, string, error)
	Add() JavaAccountAdd
	BeginAdd(ctx context.Context) error
	CancelAdd()
	Select(id string) error
	Remove(ctx context.Context, id string) error
	// PicturesPending reports pictures being read, which a later list returns.
	PicturesPending() bool
}

// javaAccountsResultV1 answers every java_accounts.v1 action with the state after it.
type javaAccountsResultV1 struct {
	SchemaVersion uint32         `json:"schema_version"`
	Accounts      []JavaAccount  `json:"accounts"`
	Selected      string         `json:"selected,omitempty"`
	Add           JavaAccountAdd `json:"add"`
	// PicturesPending asks the caller to list again for pictures being read.
	PicturesPending bool `json:"pictures_pending,omitempty"`
}

// SetJavaAccounts enables java_accounts.v1.
func (server *Server) SetJavaAccounts(accounts JavaAccounts) {
	server.mu.Lock()
	server.javaAccounts = accounts
	server.mu.Unlock()
}

func (server *Server) serveJavaAccounts(conn net.Conn, id uint64, raw json.RawMessage) error {
	reply := responseWriter{server: server, conn: conn, id: id}
	server.mu.Lock()
	accounts := server.javaAccounts
	server.mu.Unlock()
	if accounts == nil {
		return server.writeResponse(conn, response{JSONRPC: bridgecontract.JSONRPCVersion, ID: id, Error: &responseError{Code: bridgecontract.RPCMethodNotFound, Message: "Method not found"}})
	}
	var params struct {
		Action string `json:"action"`
		ID     string `json:"id,omitempty"`
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&params); err != nil || decoder.Decode(new(any)) != io.EOF {
		return reply.invalid()
	}
	ctx, cancel := context.WithTimeout(context.Background(), javaAccountsTimeout)
	defer cancel()
	var err error
	switch params.Action {
	case bridgecontract.JavaAccountsList:
	case bridgecontract.JavaAccountsAdd:
		// A failed start is reported in the add state.
		_ = accounts.BeginAdd(ctx)
	case bridgecontract.JavaAccountsCancel:
		accounts.CancelAdd()
	case bridgecontract.JavaAccountsSelect:
		err = accounts.Select(params.ID)
	case bridgecontract.JavaAccountsRemove:
		err = accounts.Remove(ctx, params.ID)
	default:
		return reply.invalid()
	}
	if errors.Is(err, ErrNoJavaAccount) {
		return reply.invalid()
	}
	if err != nil {
		server.logServiceFailure(methodJavaAccounts, err)
		return reply.fail(codeServiceFailed, "Service unavailable")
	}
	list, selected, err := accounts.List()
	if err != nil {
		server.logServiceFailure(methodJavaAccounts, err)
		return reply.fail(codeServiceFailed, "Service unavailable")
	}
	if list == nil {
		list = []JavaAccount{}
	}
	return reply.ok(javaAccountsResultV1{
		SchemaVersion: bridgecontract.ControlSchemaVersion,
		Accounts:      list,
		Selected:      selected,
		Add:           accounts.Add(),
		// Read after listing, which starts the reads of missing pictures.
		PicturesPending: accounts.PicturesPending(),
	})
}
