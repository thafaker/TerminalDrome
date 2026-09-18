use keyring::Entry;
use anyhow::Result;

const KEYRING_SERVICE: &str = "terminaldrome";

/// Liest das Passwort/Token für einen bestimmten User aus dem System-Keyring
pub fn get_password_from_keyring(username: &str) -> Option<String> {
    let entry = Entry::new(KEYRING_SERVICE, username).ok()?;
    entry.get_password().ok()
}

/// Speichert das Passwort/Token im System-Keyring
pub fn set_password_in_keyring(username: &str, password: &str) -> Result<()> {
    let entry = Entry::new(KEYRING_SERVICE, username)?;
    entry.set_password(password)?;
    Ok(())
}

/// Löscht das Passwort/Token aus dem Keyring (z. B. bei Logout oder fehlerhaftem Login)
pub fn delete_password_from_keyring(username: &str) -> Result<()> {
    let entry = Entry::new(KEYRING_SERVICE, username)?;
    entry.delete_password()?;
    Ok(())
}

// 1. Passwort-Priorität durchgehen
let mut password = args.password
    .or_else(|| config.as_ref().and_then(|c| {
        if c.server.password.is_empty() { None } else { Some(c.server.password.clone()) }
    }));

// 2. Falls kein Passwort in CLI/Config, aus dem Keyring lesen
if password.is_none() && !username.is_empty() {
    if let Some(keyring_pass) = get_password_from_keyring(&username) {
        password = Some(keyring_pass);
    }
}

// 3. Falls immer noch kein Passwort vorhanden, interaktiv erfragen
if password.is_none() {
    let prompt_pass = rpassword::prompt_password("Enter Password/Token: ")?;
    if !prompt_pass.is_empty() {
        // Fragt kurz, ob das Passwort für die Zukunft im Keyring gespeichert werden soll
        print!("Save password to system keyring? [Y/n]: ");
        io::stdout().flush()?;
        let mut save_input = String::new();
        io::stdin().read_line(&save_input)?;
        if save_input.trim().to_lowercase() != "n" {
            if let Err(e) = set_password_in_keyring(&username, &prompt_pass) {
                eprintln!("Warning: Could not save password to keyring: {}", e);
            }
        }
        password = Some(prompt_pass);
    }
}

let password = password.unwrap_or_default();