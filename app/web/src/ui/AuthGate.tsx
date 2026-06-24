import { FormEvent, useEffect, useMemo, useState } from "react";
import { KeyRound, LockKeyhole, UserPlus } from "lucide-react";
import { ApiClient } from "../api/client";
import type { AuthCapabilities } from "../api/types";

type AuthMode = "login" | "signup" | "legacy";

type AuthGateProps = {
  onAuthenticated: (token: string) => void;
};

export function AuthGate({ onAuthenticated }: AuthGateProps) {
  const client = useMemo(() => new ApiClient(""), []);
  const [mode, setMode] = useState<AuthMode>("login");
  const [capabilities, setCapabilities] = useState<AuthCapabilities | null>(null);
  const [capabilityError, setCapabilityError] = useState("");
  const [status, setStatus] = useState("");
  const [loginDraft, setLoginDraft] = useState({ email: "", password: "" });
  const [signupDraft, setSignupDraft] = useState({ displayName: "", email: "", password: "", bootstrapToken: "" });
  const [legacyToken, setLegacyToken] = useState("");

  useEffect(() => {
    client.authCapabilities()
      .then((data) => {
        setCapabilities(data);
        setCapabilityError("");
      })
      .catch((error: Error) => {
        setCapabilities(null);
        setCapabilityError(error.message);
      });
  }, [client]);

  const emailPasswordEnabled = capabilities?.email_password?.available !== false;
  const signupEnabled = capabilities?.signup_enabled !== false;
  const ssoProviders = capabilities?.sso ?? [];
  const ssoEnabled = ssoProviders.some((provider) => provider.available && provider.configured);

  function submitLogin(event: FormEvent) {
    event.preventDefault();
    const email = loginDraft.email.trim();
    if (!email || !loginDraft.password) return;
    setStatus("Signing in...");
    client.login({ email, password: loginDraft.password, device_label: browserDeviceName() })
      .then((session) => onAuthenticated(session.session_token))
      .catch((error: Error) => setStatus(error.message));
  }

  function submitSignup(event: FormEvent) {
    event.preventDefault();
    const email = signupDraft.email.trim();
    const displayName = signupDraft.displayName.trim();
    const bootstrapToken = signupDraft.bootstrapToken.trim();
    if (!email || !displayName || !signupDraft.password || !bootstrapToken) return;
    setStatus("Creating account...");
    client.signup({
      email,
      password: signupDraft.password,
      display_name: displayName,
      bootstrap_token: bootstrapToken,
      device_label: browserDeviceName()
    }).then((session) => onAuthenticated(session.session_token))
      .catch((error: Error) => setStatus(error.message));
  }

  function submitLegacy(event: FormEvent) {
    event.preventDefault();
    const token = legacyToken.trim();
    if (token) onAuthenticated(token);
  }

  return (
    <main className="auth-wrap">
      <section className="auth-card">
        <div className="brand-lockup">
          <div className="mark">c</div>
          <div>
            <b>coca</b>
            <span>coder-agent session workspace</span>
          </div>
        </div>
        <div className="auth-tabs" role="tablist" aria-label="Sign in method">
          <button className={mode === "login" ? "active" : ""} type="button" onClick={() => setMode("login")}>Sign in</button>
          <button className={mode === "signup" ? "active" : ""} type="button" onClick={() => setMode("signup")}>First user</button>
          <button className={mode === "legacy" ? "active" : ""} type="button" onClick={() => setMode("legacy")}>Gateway token</button>
        </div>
        {mode === "login" && (
          <>
            <h1>Sign in to your workspace</h1>
            <p>Use your account credentials for session browsing and profile security. Terminal write actions still require a separate daemon-backed token.</p>
            <form onSubmit={submitLogin}>
              <label className="field">
                <span>Email</span>
                <input type="email" value={loginDraft.email} onChange={(event) => setLoginDraft({ ...loginDraft, email: event.target.value })} autoFocus autoComplete="email" disabled={!emailPasswordEnabled} />
              </label>
              <label className="field">
                <span>Password</span>
                <input type="password" value={loginDraft.password} onChange={(event) => setLoginDraft({ ...loginDraft, password: event.target.value })} autoComplete="current-password" disabled={!emailPasswordEnabled} />
              </label>
              <button className="btn primary" type="submit" disabled={!emailPasswordEnabled}><LockKeyhole size={16} /> Sign in</button>
            </form>
          </>
        )}
        {mode === "signup" && (
          <>
            <h1>Create the first account</h1>
            <p>Use the bootstrap/share token once to register the first workspace user on this gateway.</p>
            <form onSubmit={submitSignup}>
              <label className="field"><span>Display name</span><input value={signupDraft.displayName} onChange={(event) => setSignupDraft({ ...signupDraft, displayName: event.target.value })} autoComplete="name" disabled={!signupEnabled} /></label>
              <label className="field"><span>Email</span><input type="email" value={signupDraft.email} onChange={(event) => setSignupDraft({ ...signupDraft, email: event.target.value })} autoComplete="email" disabled={!signupEnabled} /></label>
              <label className="field"><span>Password</span><input type="password" value={signupDraft.password} onChange={(event) => setSignupDraft({ ...signupDraft, password: event.target.value })} autoComplete="new-password" disabled={!signupEnabled} /></label>
              <label className="field"><span>Bootstrap token</span><input type="password" value={signupDraft.bootstrapToken} onChange={(event) => setSignupDraft({ ...signupDraft, bootstrapToken: event.target.value })} autoComplete="one-time-code" disabled={!signupEnabled} /></label>
              <button className="btn primary" type="submit" disabled={!signupEnabled}><UserPlus size={16} /> Create account</button>
            </form>
          </>
        )}
        {mode === "legacy" && (
          <>
            <h1>Use a local gateway token</h1>
            <p>Legacy token mode keeps account features unavailable and preserves terminal token entry as a separate control.</p>
            <form onSubmit={submitLegacy}>
              <label className="field">
                <span>Gateway access token</span>
                <input type="password" value={legacyToken} onChange={(event) => setLegacyToken(event.target.value)} autoComplete="current-password" />
              </label>
              <button className="btn primary" type="submit"><KeyRound size={16} /> Continue</button>
            </form>
          </>
        )}
        <div className="sso-row" aria-label="SSO providers">
          {["GitHub", "Google"].map((provider) => (
            <button className="btn" type="button" disabled={!ssoEnabled || !ssoProviderEnabled(ssoProviders, provider)} key={provider}>
              SSO: {provider}
            </button>
          ))}
        </div>
        <div className="auth-switch">
          {capabilityError ? `Capabilities unavailable: ${capabilityError}` : ssoEnabled ? "SSO is available from this gateway." : "SSO is not configured on this gateway."}
          {status && <div className="save-status">{status}</div>}
        </div>
      </section>
      <aside className="auth-side">
        <div>
          <h2>Runtime boundary is explicit</h2>
          <p>The browser is a workbench. The gateway authorizes API and socket access; the daemon owns terminal processes.</p>
          <div className="boundary">
            <div className="node"><b>Browser</b><span>Read transcripts, request attaches, inspect provenance.</span></div>
            <div className="arrow" />
            <div className="node"><b>Gateway</b><span>Authenticates account or legacy token requests and forwards runtime streams.</span></div>
            <div className="arrow" />
            <div className="node"><b>Daemon</b><span>Owns terminal lifecycle and provider CLI processes.</span></div>
          </div>
        </div>
        <div className="auth-log">
          <div>scope: transcript.read sessions.browse</div>
          <div>account: profile security devices tokens</div>
          <div>write: terminal actions require daemon token</div>
        </div>
      </aside>
    </main>
  );
}

function browserDeviceName() {
  const browser = navigator.userAgent.includes("Firefox") ? "Firefox" : navigator.userAgent.includes("Safari") && !navigator.userAgent.includes("Chrome") ? "Safari" : "Chrome";
  return `${navigator.platform || "browser"} ${browser}`;
}

function ssoProviderEnabled(providers: NonNullable<AuthCapabilities["sso"]>, provider: string) {
  const target = provider.toLowerCase();
  return providers.some((item) => item.provider.toLowerCase() === target && item.available && item.configured);
}
