//! OS credential store (macOS Keychain / Windows Credential Manager). No plaintext fallback.

use keyring::Entry;

const SERVICE: &str = "nl.canvasfacilitator.app";
pub const API_KEY: &str = "openai-api-key";
pub const DB_KEY: &str = "database-key";

fn entry(name: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, name).map_err(|e| format!("Credentialopslag niet beschikbaar: {e}"))
}

pub fn get(name: &str) -> Result<Option<String>, String> {
    match entry(name)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Credentialopslag vergrendeld of niet beschikbaar: {e}")),
    }
}

pub fn set(name: &str, value: &str) -> Result<(), String> {
    entry(name)?.set_password(value).map_err(|e| format!("Opslaan in credentialopslag mislukt: {e}"))
}

pub fn delete(name: &str) -> Result<(), String> {
    match entry(name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Verwijderen uit credentialopslag mislukt: {e}")),
    }
}

/// Store and read back, so we never rely on a value the store did not actually keep.
pub fn set_verified(name: &str, value: &str) -> Result<(), String> {
    set(name, value)?;
    match get(name)? {
        Some(v) if v == value => Ok(()),
        _ => Err("De credentialopslag heeft de waarde niet bewaard".into()),
    }
}

/// Database key, created on first use. 32 random bytes as hex.
pub fn db_key() -> Result<String, String> {
    if let Some(k) = get(DB_KEY)? {
        return Ok(k);
    }
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| format!("Geen willekeurige bron: {e}"))?;
    let k: String = b.iter().map(|x| format!("{x:02x}")).collect();
    set_verified(DB_KEY, &k)?;
    Ok(k)
}

/// "sk-…abcd" style mask; the renderer never sees more.
pub fn mask(key: &str) -> String {
    let tail: String = key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{}…{tail}", key.chars().take(3).collect::<String>())
}

/// Replace anything that looks like an OpenAI key. Used for every diagnostics line.
pub fn redact(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("sk-") {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(tail.len());
        out.push_str("[REDACTED]");
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn mask_and_redact() {
        assert_eq!(super::mask("sk-proj-abcdef1234"), "sk-…1234");
        assert_eq!(super::redact("auth sk-proj-AbC_12-x failed; sk-z"), "auth [REDACTED] failed; [REDACTED]");
    }
}
