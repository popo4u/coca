import { FormEvent, useEffect, useState } from "react";
import { KeyRound, ShieldCheck, UserRound } from "lucide-react";
import type { ApiClient } from "../api/client";
import type { AccountDevice, AccountMe, AccountToken, AccountUser } from "../api/types";

type ProfileViewProps = {
  client: ApiClient;
  account: AccountMe | null;
  legacyReason?: string;
  onUserChange: (user: AccountUser) => void;
};

type LoadState<T> =
  | { status: "loading" }
  | { status: "ready"; data: T }
  | { status: "error"; error: string };

export function ProfileView({ client, account, legacyReason, onUserChange }: ProfileViewProps) {
  const [profileDraft, setProfileDraft] = useState({ displayName: account?.user.display_name ?? "", email: account?.user.email ?? "" });
  const [passwordDraft, setPasswordDraft] = useState({ currentPassword: "", newPassword: "" });
  const [tokenName, setTokenName] = useState("");
  const [profileStatus, setProfileStatus] = useState("");
  const [passwordStatus, setPasswordStatus] = useState("");
  const [tokenStatus, setTokenStatus] = useState("");
  const [createdToken, setCreatedToken] = useState<{ name: string; accessToken: string } | null>(null);
  const [tokens, setTokens] = useState<LoadState<AccountToken[]>>({ status: "loading" });
  const [devices, setDevices] = useState<LoadState<AccountDevice[]>>({ status: "loading" });

  useEffect(() => {
    setProfileDraft({ displayName: account?.user.display_name ?? "", email: account?.user.email ?? "" });
  }, [account?.user.display_name, account?.user.email]);

  useEffect(() => {
    if (!account) return;
    refreshTokens();
    refreshDevices();
  }, [account, client]);

  if (!account) {
    return (
      <div className="profile-stack">
        <section className="module">
          <div className="module-body profile-head">
            <div className="big-avatar">lg</div>
            <div>
              <h2 className="entity-title">Local gateway token</h2>
              <div className="muted">Account identity is unavailable in legacy token mode.</div>
              <div className="badge-row"><span className="status-badge warning">legacy auth</span><span className="tag">terminal token separate</span></div>
            </div>
          </div>
        </section>
        <section className="module">
          <header className="module-head"><h2 className="module-title"><ShieldCheck size={16} /> Profile unavailable</h2></header>
          <div className="module-body">
            <div className="notice warning"><b>Account APIs did not confirm this token.</b><br />{legacyReason || "Use email/password login to manage profile, access tokens, and device sessions."}</div>
            <p className="muted">The browser remains a gateway API client. Terminal access continues to use the explicit terminal token controls in Active Terminals and session detail.</p>
          </div>
        </section>
      </div>
    );
  }

  const initials = accountInitials(account.user);

  function refreshTokens() {
    setTokens({ status: "loading" });
    client.accountTokens()
      .then((data) => setTokens({ status: "ready", data: data.tokens }))
      .catch((error: Error) => setTokens({ status: "error", error: error.message }));
  }

  function refreshDevices() {
    setDevices({ status: "loading" });
    client.accountDevices()
      .then((data) => setDevices({ status: "ready", data: data.devices }))
      .catch((error: Error) => setDevices({ status: "error", error: error.message }));
  }

  function saveProfile(event: FormEvent) {
    event.preventDefault();
    const displayName = profileDraft.displayName.trim();
    const email = profileDraft.email.trim();
    if (!displayName) return;
    setProfileStatus("Saving...");
    client.updateProfile({ display_name: displayName, email: email || undefined })
      .then((user) => {
        onUserChange(user);
        setProfileStatus("Profile saved.");
      })
      .catch((error: Error) => setProfileStatus(error.message));
  }

  function savePassword(event: FormEvent) {
    event.preventDefault();
    if (!passwordDraft.currentPassword || !passwordDraft.newPassword) return;
    setPasswordStatus("Saving...");
    client.updatePassword({ current_password: passwordDraft.currentPassword, new_password: passwordDraft.newPassword })
      .then(() => {
        setPasswordDraft({ currentPassword: "", newPassword: "" });
        setPasswordStatus("Password changed.");
      })
      .catch((error: Error) => setPasswordStatus(error.message));
  }

  function createToken(event: FormEvent) {
    event.preventDefault();
    const name = tokenName.trim();
    if (!name) return;
    setTokenStatus("Creating...");
    client.createAccountToken(name)
      .then((data) => {
        setCreatedToken({ name, accessToken: data.plaintext_token || data.access_token || "" });
        setTokenName("");
        setTokenStatus("Token created.");
        refreshTokens();
      })
      .catch((error: Error) => setTokenStatus(error.message));
  }

  function revokeToken(token: AccountToken) {
    const id = tokenId(token);
    if (!id) return;
    setTokenStatus("Revoking...");
    client.revokeAccountToken(id)
      .then(() => {
        setTokenStatus("Token revoked.");
        refreshTokens();
      })
      .catch((error: Error) => setTokenStatus(error.message));
  }

  function revokeDevice(device: AccountDevice) {
    const id = deviceId(device);
    if (!id) return;
    client.revokeDevice(id)
      .then(refreshDevices)
      .catch((error: Error) => setDevices({ status: "error", error: error.message }));
  }

  return (
    <div className="profile-stack">
      <section className="module">
        <div className="module-body profile-head">
          <div className="big-avatar">{initials}</div>
          <div>
            <h2 className="entity-title">{account.user.display_name || account.user.email}</h2>
            <div className="muted truncate">{account.user.email}{account.user.created_at_ms ? ` · Joined ${formatDate(account.user.created_at_ms)}` : ""}</div>
            <div className="badge-row"><span className="tag">transcript.read</span><span className="tag">account.manage</span><span className="tag">terminal token separate</span></div>
          </div>
          <span className="status-badge success">{account.auth_mode ?? "account"}</span>
        </div>
      </section>
      <div className="grid-12">
        <section className="module span-6">
          <header className="module-head"><h2 className="module-title"><UserRound size={16} /> Profile</h2></header>
          <div className="module-body">
            <form className="account-form" onSubmit={saveProfile}>
              <label className="field"><span>Display name</span><input value={profileDraft.displayName} onChange={(event) => setProfileDraft({ ...profileDraft, displayName: event.target.value })} /></label>
              <label className="field"><span>Email</span><input type="email" value={profileDraft.email} onChange={(event) => setProfileDraft({ ...profileDraft, email: event.target.value })} /></label>
              <div className="panel-actions"><button className="btn primary small-btn" type="submit">Save profile</button>{profileStatus && <span className="save-status">{profileStatus}</span>}</div>
            </form>
          </div>
        </section>
        <section className="module span-6">
          <header className="module-head"><h2 className="module-title"><KeyRound size={16} /> Password</h2></header>
          <div className="module-body">
            <form className="account-form" onSubmit={savePassword}>
              <label className="field"><span>Current password</span><input type="password" value={passwordDraft.currentPassword} onChange={(event) => setPasswordDraft({ ...passwordDraft, currentPassword: event.target.value })} autoComplete="current-password" /></label>
              <label className="field"><span>New password</span><input type="password" value={passwordDraft.newPassword} onChange={(event) => setPasswordDraft({ ...passwordDraft, newPassword: event.target.value })} autoComplete="new-password" /></label>
              <div className="panel-actions"><button className="btn primary small-btn" type="submit">Change password</button>{passwordStatus && <span className="save-status">{passwordStatus}</span>}</div>
            </form>
          </div>
        </section>
      </div>
      <section className="module">
        <header className="module-head">
          <h2 className="module-title"><ShieldCheck size={16} /> Security / Access</h2>
          <form className="inline-create" onSubmit={createToken}>
            <input value={tokenName} onChange={(event) => setTokenName(event.target.value)} placeholder="Token name" aria-label="Token name" />
            <button className="btn small-btn" type="submit">Create token</button>
          </form>
        </header>
        <div className="module-body">
          <div className="notice"><b>Terminal access stays separate.</b><br />Account access tokens authenticate gateway APIs. Terminal write operations still require a terminal token and daemon runtime readiness.</div>
          {createdToken && (
            <div className="one-time-token">
              <div><b>{createdToken.name}</b><span>Copy this value now. It will not be shown again.</span></div>
              <code>{createdToken.accessToken}</code>
              <button className="btn small-btn" type="button" onClick={() => setCreatedToken(null)}>Dismiss</button>
            </div>
          )}
          {tokens.status === "loading" && <p className="muted">Loading access tokens...</p>}
          {tokens.status === "error" && <div className="notice error"><b>Access tokens unavailable</b><br />{tokens.error}</div>}
          {tokens.status === "ready" && (
            <div className="access-list">
              {tokens.data.map((token) => {
                const lastUsed = tokenLastUsed(token);
                return (
                  <div className="token-row" key={tokenId(token) || token.name}>
                    <div className="truncate"><b>{token.name}</b><div className="small muted mono truncate">{tokenPreview(token)}</div></div>
                    <span className="tag">gateway</span>
                    <span className="small muted">{lastUsed ? `Last used ${formatDate(lastUsed)}` : "Never used"}</span>
                    <button className="btn danger small-btn" type="button" onClick={() => revokeToken(token)} disabled={!tokenId(token)}>Revoke</button>
                  </div>
                );
              })}
              {tokens.data.length === 0 && <p className="muted">No account access tokens have been created.</p>}
            </div>
          )}
          {tokenStatus && <p className="save-status">{tokenStatus}</p>}
        </div>
      </section>
      <section className="module">
        <header className="module-head"><h2 className="module-title"><UserRound size={16} /> Device / Browser Sessions</h2></header>
        <div className="module-body">
          {devices.status === "loading" && <p className="muted">Loading devices...</p>}
          {devices.status === "error" && <div className="notice error"><b>Device sessions unavailable</b><br />{devices.error}</div>}
          {devices.status === "ready" && (
            <div className="access-list">
              {devices.data.map((device) => {
                const current = device.current || deviceId(device) === deviceId(account.device ?? {});
                const lastSeen = deviceLastSeen(device);
                return (
                  <div className="device-row" key={deviceId(device) || deviceLabel(device)}>
                    <div className="truncate"><b>{deviceLabel(device)}</b><div className="small muted truncate">{device.ip || "Gateway session"}{device.user_agent ? ` · ${device.user_agent}` : ""}</div></div>
                    <span className={`status-badge ${current ? "success" : "info"}`}>{current ? "current" : "ready"}</span>
                    <span className="small muted">{lastSeen ? formatDate(lastSeen) : "Unknown"}</span>
                    <button className={`btn ${current ? "" : "danger"} small-btn`} type="button" disabled={current || !deviceId(device)} onClick={() => revokeDevice(device)}>{current ? "Current" : "Revoke"}</button>
                  </div>
                );
              })}
              {devices.data.length === 0 && <p className="muted">No browser sessions were returned by the gateway.</p>}
            </div>
          )}
        </div>
      </section>
    </div>
  );
}

function accountInitials(user: AccountUser) {
  const source = user.display_name || user.email || "coca";
  return source.split(/[^\p{L}\p{N}]+/u).filter(Boolean).slice(0, 2).map((part) => part[0]).join("").toLowerCase() || "cx";
}

function tokenId(token: AccountToken) {
  return token.id ?? token.token_id ?? "";
}

function tokenPreview(token: AccountToken) {
  const preview = token.preview ?? token.token_preview;
  if (preview) return preview;
  return token.id ? `tok_...${token.id.slice(-6)}` : "token preview unavailable";
}

function deviceId(device: AccountDevice) {
  return device.id ?? device.device_id ?? "";
}

function deviceLabel(device: AccountDevice) {
  return device.label ?? device.name ?? device.device_name ?? "Browser session";
}

function tokenLastUsed(token: AccountToken) {
  return token.last_used_at_ms ?? token.last_used_at ?? null;
}

function deviceLastSeen(device: AccountDevice) {
  return device.last_seen_at_ms ?? device.last_seen_at ?? null;
}

function formatDate(value: string | number) {
  const date = typeof value === "number" ? new Date(value) : new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date);
}
