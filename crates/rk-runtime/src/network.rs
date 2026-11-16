use std::net::{Ipv4Addr, UdpSocket};

/// This computer's address on the local network: the source address the OS
/// would use to reach a public address. A UDP `connect` sends nothing.
pub fn lan_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?; // TEST-NET-1
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

/// Name of the program listening on TCP `port` on this computer (e.g.
/// `authserver.exe`), to say which one to close when a port is taken.
/// Windows only (`netstat` then `tasklist`); `None` when unknown.
pub fn port_owner(port: u16) -> Option<String> {
    #[cfg(windows)]
    {
        let netstat = hidden_output("netstat", &["-ano", "-p", "TCP"])?;
        let pid = listening_pid(&netstat, port)?;
        let filter = format!("PID eq {pid}");
        let tasks = hidden_output("tasklist", &["/FI", &filter, "/FO", "CSV", "/NH"])?;
        process_name(&tasks)
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        None
    }
}

#[cfg(windows)]
fn hidden_output(program: &str, args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// PID of the socket listening on `port` in `netstat -ano` output. A
/// listening socket has no remote end (`0.0.0.0:0`, `[::]:0`); the state
/// column is translated, so it is not read.
#[cfg_attr(not(windows), allow(dead_code))]
fn listening_pid(netstat: &str, port: u16) -> Option<u32> {
    let suffix = format!(":{port}");
    netstat.lines().find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields.as_slice() {
            ["TCP", local, remote, .., pid]
                if local.ends_with(&suffix) && remote.ends_with(":0") =>
            {
                pid.parse().ok().filter(|p| *p != 0)
            }
            _ => None,
        }
    })
}

/// First field of `tasklist /FO CSV /NH` output.
#[cfg_attr(not(windows), allow(dead_code))]
fn process_name(tasklist: &str) -> Option<String> {
    let line = tasklist.lines().find(|l| l.starts_with('"'))?;
    let name = line.trim_start_matches('"').split('"').next()?;
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listening_socket_gives_the_pid() {
        let netstat = "\r\nConnexions actives\r\n\r\n  Proto  Adresse locale         Adresse distante       État\r\n\
            \x20 TCP    127.0.0.1:53724        127.0.0.1:3724         ESTABLISHED     900\r\n\
            \x20 TCP    0.0.0.0:3724           0.0.0.0:0              LISTENING       4242\r\n\
            \x20 TCP    [::]:8085              [::]:0                 LISTENING       77\r\n";
        assert_eq!(listening_pid(netstat, 3724), Some(4242));
        assert_eq!(listening_pid(netstat, 8085), Some(77));
        assert_eq!(listening_pid(netstat, 372), None);
        assert_eq!(listening_pid(netstat, 7878), None);
    }

    #[test]
    fn the_process_name_is_the_first_csv_field() {
        assert_eq!(
            process_name("\"authserver.exe\",\"4242\",\"Console\",\"1\",\"12 345 Ko\"\r\n")
                .as_deref(),
            Some("authserver.exe")
        );
        assert_eq!(process_name("INFO: aucune tâche ne correspond.\r\n"), None);
    }
}
