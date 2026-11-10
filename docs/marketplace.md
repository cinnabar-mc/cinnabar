# Marketplace installation

The in-game marketplace entry points are hidden until the marketplace launches
publicly. Signed installs, library sync, revocation checks and updates remain available.
Sign in with the same Xbox account in the client and on the website, then choose
**Add to Cinnabar** on a mod page. New downloads appear under
**My mods** and stay disabled until you choose **Enable** and review permissions.
Account guidance explains sign-in; delivery messages report preparation and
download failures separately. Local imports remain available without sign-in.

The website's Add to Cinnabar action queues a release for the signed-in Xbox
account. The account core receives the library in the authenticated API welcome
and subsequent `mod_library` messages. `cinnabar_mods.v1` exposes only that live
account's bounded selectors to the native client; it carries no access tokens or
permission authority. The API schema and its immutable revision are recorded in
`core/cinnabarapi/schema/source.json`.

The native marketplace worker uses the API origin from the generated bridge
contract. It fetches the exact signed release by project, version and SHA-256, verifies
the original statement bytes with a pinned Ed25519 public key, checks the
selection/size/hash, and retrieves the component through its digest URL. Public
requests carry no account credentials and follow no redirects. Responses,
component bytes and cached metadata are bounded; account changes cancel pending
work. Returning to an account retries selections that were not installed. Verified
receipts retain successful installations and local removals across account changes.
The account and exact selection are confirmed again after downloads. Selections
held by account, freshness or withdrawal are not downloaded. A send whose
preparation fails (fetch, verification or a full installer queue) is retried
after 30 seconds, doubling to at most 30 minutes, until it is prepared, the
account changes or the player sends it again. A prepared send counts as
delivered once the library worker accepts it; if that worker's compatibility
check or installation (file or receipt publication) then fails, the send is not
retried until the account changes or the client restarts. Component downloads
allow time for their signed size on slow connections, within a fixed ceiling.

The existing component host checks ABI compatibility without executing uploaded
code. The component's declared version and exact requested grants, including
command values, must match the signed release. Components and signed receipts
are published under immutable, account/hash-derived names; each file is complete
before it becomes visible. Fresh deliveries never overwrite existing files, enable a mod,
copy prior grants or authorize new ones. A newly published file drops any
registration entry left at its path, for example after the file was deleted
outside Mods, so a re-sent release needs a fresh Enable. New releases appear under Settings →
Mods → My mods; enable them and choose permissions through the existing consent
controls. A marketplace mod never runs with more than its signed grants: the host
caps its saved permissions to the signed set, including command names, whenever
it is admitted, and the consent controls offer only the requested permissions, so
a player can turn signed capabilities off but cannot add others. Updates publish new immutable files and receipts before retiring predecessors.
The official mod's revision updater cannot replace managed files. Component-declared
update repositories, URLs and revision identities are ignored for every other local
mod; only the official package uses the revision-checked `/v1/mod/update` path.

## Automatic updates

While signed into the owning account, the worker checks installed signed receipts
once a minute. `GET /v1/mods/{slug}/update` sends `api_revisions` (the same comma-separated
supported revisions as the official updater), `host_revision`, `current_version`
and `current_sha256`. A 200 response contains `release.statement` and `release.signature`;
204 and 404 leave the installation unchanged. Updates require a signed supported
`api_revision`, the same project, a strictly newer version, matching size and SHA-256,
valid native declarations and grants, current account ownership and fresh revocation
evidence. No redirects or account credentials are sent to the public API.

Settings → Updates controls mod checks and automatic installation. When automatic
installation is off, compatible releases appear in the existing Home notice and Mods
Update button. Turning checks off also cancels prepared updates. Failed attempts,
including installer handoffs without a published successor, use delivery backoff.

If signed permissions stay the same or shrink, the successor receipt first records
pending migration. An admission fence stops the predecessor's runtime without
changing its retained enabled selection. After its final settings writes drain,
the client atomically replaces the successor's settings companion, then transfers
enabled state, order and permission choices capped to the new signed set in one
registration write. Pre-transfer failures release the fence; the predecessor stays
selected. After transfer, cleanup removes the old settings, component and receipt,
then clears the migration marker. Retries after transfer preserve newer successor
saves. Periodic checks, manual requests and restarts resume interrupted migration.
Declined permissions remain declined; command names are capped individually. If permissions expand, the
new version stays disabled in My mods and the old version keeps running. Enabling
the successor validates it again and retires older versions before using the existing
permission consent controls. The successor receipt retains consumed website sends
so restarting cannot reinstall a replaced release. Removed successors retain those
consumed sends for authenticated removal.

## Withdrawal and offline behavior

Signed revocation snapshots include issue and expiry timestamps. The API issues
fifteen-minute snapshots; while a managed mod is installed or the signed-in
account has selections, the client refreshes every minute, rejects expired,
future-dated or rolled-back evidence, and persists the last signed checkpoint.
Refresh failures appear in Mods only when they affect an installed managed mod
or a pending selection. Without fresh evidence, managed mods stay held.
Withdrawn hashes seen during a session stay held until the client exits. After a
restart the client starts from the persisted latest snapshot, so a hash that
snapshot no longer lists is no longer held. Expired cached snapshots retain their issue time across restarts but
never grant a fresh execution window.

Within an established account session, a network failure does not extend the
remaining signed window. Managed mods are held when that window expires, the
account changes, a withdrawal arrives, or the receipt/source is invalid. A new
client account session must establish its identity through the authenticated API.
Source admission, prepared-instance publication and frame authority all check
the current guard. Local developer imports keep their existing explicit grants.
Managed filenames use a reserved `cinnabar-marketplace-` prefix and remain
reserved even if a receipt is deleted; other local file names are unaffected.

Removing a managed mod through Mods leaves its signed receipt as a consumed
request. The worker queues `mod_library_remove` through the authenticated game
connection until the server drops that exact account/project/hash/send timestamp,
waiting the same backoff as a failed send between requests.
It cannot remove another account's selection or erase a later website send. A
new Add to Cinnabar action can intentionally reinstall the release.

## Production configuration and acceptance

Populate `crates/launcher-host/data/marketplace-keys.json` with reviewed base64
encodings of the raw 32-byte Ed25519 public keys corresponding to the API signer.
The initial public key is provisioned alongside the API's encrypted GitHub
`production` signing secret. The API deployment checks that its signer matches
the immutable client input before installing it. Deploy that API before shipping
the client key pin; unverified responses keep managed files held. Never put
the private signing key in the client, trust a key supplied by a marketplace
response, or reuse test keys. Rotation ships overlapping reviewed public keys
before the server changes its signer.

Deploy the API with compatible update lookup, signed API revisions, exact-release retrieval, expiring signed revocations and
authenticated conditional removals before enabling client downloads. Test the
real website → account connection → disabled install → explicit consent flow,
offline expiry, restart, account switches, withdrawals and filesystem failures
on supported platforms. Synthetic tests cover signature tampering, exact grant
comparison and capping, rollback/expiry, cancelled publication, collisions, consumed removals
and authenticated transport. The synthetic website/API handoff is replayed through
the native HTTP downloader, disabled installation and signed withdrawal path;
the same component exercises native ABI validation and Mods enable/consent actions.
These checks do not constitute production-domain
or interactive-client acceptance evidence.
