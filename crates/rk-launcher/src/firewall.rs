//! Windows firewall rule for hosting a realm on the local network (ADR 0022,
//! principle 6: the launcher's only elevated action, and an optional one).
//!
//! - One inbound rule per server program of the realm: TCP, that program
//!   only, limited to the port it is waited on (manifest `readiness`), for
//!   private and domain networks. Named `RealmKeeper <realm> <process>
//!   <hash>`, the hash naming the program's path and port: a realm whose
//!   folder moved shows as not allowed until the rule is made again.
//! - Made or removed by one elevated PowerShell (one UAC prompt); the
//!   script travels as `-EncodedCommand`, so no path needs quoting on a
//!   command line. Read back without elevation.
//! - Elsewhere than Windows: not supported (nothing to do on most systems).

use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

/// Group of every rule the launcher makes (Windows' "Group" column).
const GROUP: &str = "RealmKeeper";
/// Exit code of the elevation wrapper when the player declines the prompt
/// (ERROR_CANCELLED).
const DECLINED: i32 = 1223;

/// A server program other computers connect to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// Manifest process id (`worldserver`).
    pub process: String,
    pub path: PathBuf,
    /// The TCP port it listens on, when the manifest tells.
    pub port: Option<u16>,
}

/// Whether other computers of the network can reach a realm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallStatus {
    /// Rules can be made on this system.
    pub supported: bool,
    /// Every server program has its enabled rule.
    pub allowed: bool,
    /// Windows treats the network this computer uses as public: the rules
    /// (private and domain networks only) do not apply there.
    pub public_network: bool,
}

#[derive(Debug, Error)]
pub enum FirewallError {
    #[error("the administrator prompt was declined")]
    Declined,
    #[error("the firewall rule could not be changed: {0}")]
    Failed(String),
    #[error("firewall rules are only made on Windows")]
    Unsupported,
}

/// Display name of the rule of `program` in `realm`.
pub fn rule_name(realm: &str, program: &Program) -> String {
    // The port too: a rule for another port does not let the players in.
    let key = format!(
        "{}|{}",
        program.path.to_string_lossy().to_lowercase(),
        program.port.map(|p| p.to_string()).unwrap_or_default()
    );
    format!(
        "{} {:08x}",
        rule_prefix(realm, &program.process),
        fnv1a(key.as_bytes()) as u32
    )
}

fn rule_prefix(realm: &str, process: &str) -> String {
    format!("{GROUP} {realm} {process}")
}

/// What the firewall has for `realm`.
pub async fn status(realm: &str, programs: &[Program]) -> FirewallStatus {
    if !cfg!(windows) {
        return FirewallStatus {
            supported: false,
            allowed: false,
            public_network: false,
        };
    }
    let lan = rk_runtime::lan_ipv4().map(|ip| ip.to_string());
    let output = powershell(&status_script(realm, lan.as_deref()))
        .await
        .ok()
        .filter(|(code, _)| *code == 0)
        .map(|(_, out)| out)
        .unwrap_or_default();
    let (names, public_network) = parse_status(&output);
    FirewallStatus {
        supported: true,
        allowed: !programs.is_empty()
            && programs
                .iter()
                .all(|p| names.contains(&rule_name(realm, p))),
        public_network,
    }
}

/// Whether the firewall holds any rule of `realm` (enabled or not, current
/// or made for an older path). Without elevation.
pub async fn has_rules(realm: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let script = format!(
        "@(Get-NetFirewallRule -DisplayName {} -ErrorAction SilentlyContinue).Count\n",
        quote(&format!("{GROUP} {realm} *"))
    );
    powershell(&script)
        .await
        .ok()
        .and_then(|(_, out)| out.trim().parse::<u32>().ok())
        .is_some_and(|count| count > 0)
}

/// Makes the rules of `realm` (old ones of the realm are replaced). Asks for
/// elevation once.
pub async fn allow(
    realm: &str,
    programs: &[Program],
    log: &std::path::Path,
) -> Result<(), FirewallError> {
    elevated(&allow_script(realm, programs, log), log).await
}

/// Removes every rule of `realm`. Asks for elevation once.
pub async fn remove(realm: &str, log: &std::path::Path) -> Result<(), FirewallError> {
    elevated(&remove_script(realm, log), log).await
}

/// `'text'` for PowerShell: every quote doubled. PowerShell also ends a
/// single-quoted string at the typographic quotes U+2018 to U+201B (a
/// folder named `Player’s games` would otherwise break the script).
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for c in text.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

fn status_script(realm: &str, lan: Option<&str>) -> String {
    let rules = quote(&format!("{GROUP} {realm} *"));
    let profile = match lan {
        // The profile of the interface that holds the LAN address.
        Some(ip) => format!(
            "$if = (Get-NetIPAddress -IPAddress {} -ErrorAction SilentlyContinue | Select-Object -First 1).InterfaceIndex\n\
             $public = $false\n\
             if ($if) {{ $public = @(Get-NetConnectionProfile -InterfaceIndex $if -ErrorAction SilentlyContinue | Where-Object {{ \"$($_.NetworkCategory)\" -eq 'Public' }}).Count -gt 0 }}\n",
            quote(ip)
        ),
        None => "$public = $false\n".into(),
    };
    format!(
        "$ErrorActionPreference = 'SilentlyContinue'\n\
         $names = @(Get-NetFirewallRule -DisplayName {rules} | Where-Object {{ \"$($_.Enabled)\" -eq 'True' }} | ForEach-Object {{ $_.DisplayName }})\n\
         {profile}\
         foreach ($n in $names) {{ 'rule=' + $n }}\n\
         'public=' + $public\n"
    )
}

/// Rule names and the public-network flag from [`status_script`] output.
fn parse_status(output: &str) -> (Vec<String>, bool) {
    let mut names = Vec::new();
    let mut public = false;
    for line in output.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("rule=") {
            names.push(name.to_owned());
        } else if let Some(value) = line.strip_prefix("public=") {
            public = value.eq_ignore_ascii_case("true");
        }
    }
    (names, public)
}

/// Runs `body` and records the error, if any, in `log` (the elevated
/// process has no output the launcher can read).
fn logged(body: &str, log: &std::path::Path) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         try {{\n{body}exit 0\n}} catch {{\n\
         Set-Content -LiteralPath {} -Value \"$_\" -Encoding UTF8 -ErrorAction SilentlyContinue\n\
         exit 2\n}}\n",
        quote(&log.to_string_lossy())
    )
}

fn remove_lines(realm: &str) -> String {
    format!(
        "Get-NetFirewallRule -DisplayName {} -ErrorAction SilentlyContinue | Remove-NetFirewallRule\n",
        quote(&format!("{GROUP} {realm} *"))
    )
}

fn allow_script(realm: &str, programs: &[Program], log: &std::path::Path) -> String {
    let mut body = remove_lines(realm);
    for program in programs {
        let port = program
            .port
            .map(|p| format!(" -LocalPort {p}"))
            .unwrap_or_default();
        body.push_str(&format!(
            "New-NetFirewallRule -DisplayName {} -Group {} -Description {} -Direction Inbound -Action Allow -Protocol TCP{port} -Program {} -Profile Private,Domain | Out-Null\n",
            quote(&rule_name(realm, program)),
            quote(GROUP),
            quote(&format!("Lets the other computers of the local network play on the RealmKeeper realm '{realm}'.")),
            quote(&program.path.to_string_lossy()),
        ));
    }
    logged(&body, log)
}

fn remove_script(realm: &str, log: &std::path::Path) -> String {
    logged(&remove_lines(realm), log)
}

/// Runs `script` elevated (one UAC prompt) and waits for it.
async fn elevated(script: &str, log: &std::path::Path) -> Result<(), FirewallError> {
    if !cfg!(windows) {
        return Err(FirewallError::Unsupported);
    }
    let _ = std::fs::remove_file(log);
    let wrapper = format!(
        // Only a refused prompt is "declined" (ERROR_CANCELLED somewhere in
        // the exception chain); any other failure says why in the log.
        "try {{ $p = Start-Process -FilePath {ps} -Verb RunAs -Wait -PassThru -WindowStyle Hidden \
         -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{script}' }} catch {{\n\
         $e = $_.Exception\n\
         while ($e) {{ if ($e -is [System.ComponentModel.Win32Exception] -and $e.NativeErrorCode -eq {DECLINED}) {{ exit {DECLINED} }}; $e = $e.InnerException }}\n\
         Set-Content -LiteralPath {log} -Value \"$_\" -Encoding UTF8 -ErrorAction SilentlyContinue\n\
         exit 3\n}}\n\
         exit $p.ExitCode\n",
        ps = quote(&powershell_path().to_string_lossy()),
        script = encode_command(script),
        log = quote(&log.to_string_lossy()),
    );
    match powershell(&wrapper).await {
        Ok((0, _)) => Ok(()),
        Ok((DECLINED, _)) => Err(FirewallError::Declined),
        Ok((code, _)) => {
            let reason = std::fs::read_to_string(log)
                .map(|t| t.trim().trim_start_matches('\u{feff}').to_owned())
                .ok()
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| format!("PowerShell ended with code {code}"));
            Err(FirewallError::Failed(reason))
        }
        Err(e) => Err(FirewallError::Failed(e.to_string())),
    }
}

fn powershell_path() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(|root| {
            PathBuf::from(root)
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe")
        })
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("powershell.exe"))
}

/// Runs a script in a hidden PowerShell: (exit code, standard output).
async fn powershell(script: &str) -> std::io::Result<(i32, String)> {
    let mut command = tokio::process::Command::new(powershell_path());
    command
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
        .arg(encode_command(script))
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let out = command.output().await?;
    Ok((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
}

/// `-EncodedCommand` form: base64 of the UTF-16LE script.
fn encode_command(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// FNV-1a: a stable short hash (not for security).
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(process: &str, path: &str, port: Option<u16>) -> Program {
        Program {
            process: process.into(),
            path: PathBuf::from(path),
            port,
        }
    }

    #[test]
    fn quotes_double_every_kind_of_single_quote() {
        assert_eq!(quote("a'b"), "'a''b'");
        assert_eq!(quote("Player\u{2019}s"), "'Player\u{2019}\u{2019}s'");
        assert_eq!(
            quote("\u{2018}x\u{201A}\u{201B}"),
            "'\u{2018}\u{2018}x\u{201A}\u{201A}\u{201B}\u{201B}'"
        );
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // What PowerShell expects for `-EncodedCommand 'a'`.
        assert_eq!(encode_command("'a'"), "JwBhACcA");
    }

    #[test]
    fn rule_names_follow_the_program_path() {
        let a = program(
            "worldserver",
            r"D:\Realms\instances\one\bin\worldserver.exe",
            Some(8085),
        );
        let moved = program(
            "worldserver",
            r"E:\Realms\instances\one\bin\worldserver.exe",
            Some(8085),
        );
        let name = rule_name("one", &a);
        assert!(name.starts_with("RealmKeeper one worldserver "));
        assert_eq!(name.len(), "RealmKeeper one worldserver ".len() + 8);
        assert_ne!(name, rule_name("one", &moved));
        let other_port = program(
            "worldserver",
            r"D:\Realms\instances\one\bin\worldserver.exe",
            Some(8086),
        );
        assert_ne!(name, rule_name("one", &other_port));
        let upper = program(
            "worldserver",
            r"D:\REALMS\instances\one\bin\worldserver.exe",
            Some(8085),
        );
        assert_eq!(name, rule_name("one", &upper), "Windows paths ignore case");
    }

    #[test]
    fn the_allow_script_quotes_paths_and_limits_ports() {
        let programs = [
            program("authserver", r"C:\It's here\bin\authserver.exe", Some(3724)),
            program("worldserver", r"C:\It's here\bin\worldserver.exe", None),
        ];
        let script = allow_script("one", &programs, std::path::Path::new(r"C:\log.txt"));
        assert!(script.contains(r"-Program 'C:\It''s here\bin\authserver.exe'"));
        assert!(script.contains("-Protocol TCP -LocalPort 3724 -Program"));
        assert!(script.contains("-Protocol TCP -Program 'C:\\It''s here\\bin\\worldserver.exe'"));
        assert!(script.contains("-Profile Private,Domain"));
        // Old rules of the realm go first.
        let remove = script.find("Remove-NetFirewallRule").unwrap();
        assert!(remove < script.find("New-NetFirewallRule").unwrap());
        assert!(script.contains("-DisplayName 'RealmKeeper one *'"));
    }

    #[test]
    fn status_output_gives_rules_and_profile() {
        let (names, public) =
            parse_status("rule=RealmKeeper one authserver 1234abcd\r\nrule=RealmKeeper one worldserver 0badcafe\r\npublic=True\r\n");
        assert_eq!(
            names,
            [
                "RealmKeeper one authserver 1234abcd",
                "RealmKeeper one worldserver 0badcafe"
            ]
        );
        assert!(public);
        assert_eq!(parse_status("public=False\n"), (Vec::new(), false));
        assert_eq!(parse_status(""), (Vec::new(), false));
    }
}
