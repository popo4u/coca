import { FormEvent, useEffect, useMemo, useState } from "react";
import { ApiClient } from "../api/client";
import type { AuthCapabilities } from "../api/types";

type AuthMode = "login" | "signup";

type AuthGateProps = {
  onAuthenticated: (token: string, remember: boolean) => void;
};

export function AuthGate({ onAuthenticated }: AuthGateProps) {
  const client = useMemo(() => new ApiClient(""), []);
  const [mode, setMode] = useState<AuthMode>("login");
  const [capabilities, setCapabilities] = useState<AuthCapabilities | null>(null);
  const [capabilityError, setCapabilityError] = useState("");
  const [status, setStatus] = useState("");
  const [rememberSession, setRememberSession] = useState(true);
  const [loginDraft, setLoginDraft] = useState({ email: "", password: "" });
  const [signupDraft, setSignupDraft] = useState({ displayName: "", email: "", password: "", confirmPassword: "" });

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
  const signupEnabled = capabilities?.signup_enabled === true;
  const signupStatus = capabilities && !signupEnabled
    ? "First account already exists. Sign in instead."
    : capabilityError
      ? `Capabilities unavailable: ${capabilityError}`
      : "";

  function submitLogin(event: FormEvent) {
    event.preventDefault();
    const email = loginDraft.email.trim();
    if (!email || !loginDraft.password) return;
    setStatus("Signing in...");
    client.login({ email, password: loginDraft.password, device_label: browserDeviceName() })
      .then((session) => onAuthenticated(session.session_token, rememberSession))
      .catch((error: Error) => setStatus(error.message));
  }

  function submitSignup(event: FormEvent) {
    event.preventDefault();
    const email = signupDraft.email.trim();
    const displayName = signupDraft.displayName.trim();
    if (!signupEnabled) {
      setStatus(signupStatus || "Account creation is not available on this gateway.");
      return;
    }
    if (!email || !displayName || !signupDraft.password) return;
    if (signupDraft.password !== signupDraft.confirmPassword) {
      setStatus("Passwords do not match.");
      return;
    }
    setStatus("Creating account...");
    client.signup({
      email,
      password: signupDraft.password,
      display_name: displayName,
      device_label: browserDeviceName()
    }).then((session) => onAuthenticated(session.session_token, rememberSession))
      .catch((error: Error) => setStatus(error.message));
  }

  return (
    <main className="auth-wrap">
      <section className="auth-card">
        <div className="brand-lockup">
          <div className="mark">c</div>
          <div>
            <b>coca</b>
            <span>{mode === "login" ? "coder-agent session workspace" : "secure session continuity"}</span>
          </div>
        </div>
        {mode === "login" && (
          <>
            <h1>Sign in to your workspace</h1>
            <p>Open normalized agent sessions, inspect transcripts, and continue work through authorized terminal runtimes.</p>
            <form onSubmit={submitLogin}>
              <div className="field">
                <label htmlFor="auth-email">Email</label>
                <input id="auth-email" type="email" value={loginDraft.email} onChange={(event) => setLoginDraft({ ...loginDraft, email: event.target.value })} autoFocus autoComplete="email" disabled={!emailPasswordEnabled} />
              </div>
              <div className="field">
                <label htmlFor="auth-password">Password</label>
                <input id="auth-password" type="password" value={loginDraft.password} onChange={(event) => setLoginDraft({ ...loginDraft, password: event.target.value })} autoComplete="current-password" disabled={!emailPasswordEnabled} />
              </div>
              <div className="checkline">
                <RememberSession checked={rememberSession} onChange={setRememberSession} label="Remember me" />
                <button className="link-btn" type="button" onClick={() => setStatus("Password recovery is not configured on this gateway.")}>Forgot password</button>
              </div>
              <button className="btn primary" type="submit" disabled={!emailPasswordEnabled}>Sign in</button>
            </form>
            <div className="auth-switch">No account? <button className="link-btn" type="button" onClick={() => { setMode("signup"); setStatus(""); }}>Create a developer workspace</button></div>
          </>
        )}
        {mode === "signup" && (
          <>
            <h1>Create account</h1>
            <p>Set up scoped access for read-only transcripts and write-capable terminal runtime operations.</p>
            <form onSubmit={submitSignup}>
              <div className="field"><label htmlFor="signup-name">Full name</label><input id="signup-name" value={signupDraft.displayName} onChange={(event) => setSignupDraft({ ...signupDraft, displayName: event.target.value })} autoComplete="name" disabled={!signupEnabled} /></div>
              <div className="field"><label htmlFor="signup-email">Email</label><input id="signup-email" type="email" value={signupDraft.email} onChange={(event) => setSignupDraft({ ...signupDraft, email: event.target.value })} autoComplete="email" disabled={!signupEnabled} /></div>
              <div className="field"><label htmlFor="signup-password">Password</label><input id="signup-password" type="password" value={signupDraft.password} onChange={(event) => setSignupDraft({ ...signupDraft, password: event.target.value })} autoComplete="new-password" disabled={!signupEnabled} /></div>
              <div className="field"><label htmlFor="signup-confirm-password">Confirm password</label><input id="signup-confirm-password" type="password" value={signupDraft.confirmPassword} onChange={(event) => setSignupDraft({ ...signupDraft, confirmPassword: event.target.value })} autoComplete="new-password" disabled={!signupEnabled} /></div>
              <button className="btn primary" type="submit" disabled={!signupEnabled}>Create account</button>
            </form>
            {signupStatus && <div className="save-status auth-status">{signupStatus}</div>}
            <div className="auth-switch">Already have access? <button className="link-btn" type="button" onClick={() => { setMode("login"); setStatus(""); }}>Sign in</button></div>
          </>
        )}
        {status && <div className="save-status auth-status">{status}</div>}
      </section>
    </main>
  );
}

function browserDeviceName() {
  const browser = navigator.userAgent.includes("Firefox") ? "Firefox" : navigator.userAgent.includes("Safari") && !navigator.userAgent.includes("Chrome") ? "Safari" : "Chrome";
  return `${navigator.platform || "browser"} ${browser}`;
}

function RememberSession({ checked, onChange, label }: { checked: boolean; onChange: (value: boolean) => void; label: string }) {
  return (
    <label>
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
      <span>{label}</span>
    </label>
  );
}
