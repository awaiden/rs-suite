# rsnet

Interactive Linux network bandwidth and connection-per-process monitor written in Rust, inspired by `bandwhich` and `nethogs`.

## Features

- **Per-process, per-connection & remote-address views**: Attribute live TCP/UDP traffic to processes, local/remote socket pairs, and remote hosts across all active interfaces or a single interface (`-i`).
- **Live packet capture with graceful fallback**: Uses `AF_PACKET` raw sockets (Ethernet, VLAN, IPv4, IPv6, TCP, UDP) when privileged, and automatically falls back to `/proc/net/dev` + `/proc/net/{tcp,tcp6,udp,udp6}` socket attribution when unprivileged.
- **Rolling-window rates & cumulative totals**: Switch between per-second bandwidth rates and cumulative byte/bit totals (`t` / `-t`), across binary (`bin-bytes`, `bin-bits`) and SI (`si-bytes`, `si-bits`) unit families.
- **Background reverse DNS**: Optional non-blocking hostname resolution for remote addresses (`-n` / `n` to toggle).
- **Multiple output modes**: Interactive `ratatui` TUI, non-interactive `--raw` text output, and newline-delimited `--json` snapshots.

## Requirements

- Linux (reads `/proc/net/dev`, `/proc/net/{tcp,tcp6,udp,udp6}`, and `/proc/<pid>/fd/*`; uses `AF_PACKET` raw sockets for live per-packet attribution)
- `CAP_NET_RAW` / `CAP_NET_ADMIN` capabilities or `sudo` for live packet capture (runs in `PROCFS-ONLY` fallback mode without elevated privileges)
- Rust toolchain (edition 2024, MSRV 1.85) — only needed if building from source

## Install

### Quick Install (Linux x86_64 / aarch64)

```sh
curl -fsSL https://raw.githubusercontent.com/awaiden/rs-suite/main/crates/rsnet/install.sh | sh
```

Override the install directory with `RSNET_INSTALL_DIR=/usr/local/bin` or pin a release with `RSNET_VERSION=rsnet-v0.1.0`.

### Build from Source

```bash
git clone https://github.com/awaiden/rs-suite.git
cd rs-suite
cargo build --release -p rsnet

# Run in procfs-only fallback mode, or grant CAP_NET_RAW for live packet capture:
# sudo setcap cap_net_raw,cap_net_admin=eip ./target/release/rsnet
./target/release/rsnet
```

## Usage

```text
Usage: rsnet [OPTIONS]

Options:
  -i, --interface <NAME>     Network interface name to monitor (default: all active interfaces)
  -d, --delay <MS>           Refresh interval in milliseconds (100..=60000) [default: 1000]
  -r, --raw                  Run in non-interactive raw text mode instead of TUI
      --json                 Emit newline-delimited JSON snapshots
  -c, --count <N>            Stop after N refresh cycles
  -n, --no-resolve           Disable reverse DNS hostname resolution
  -u, --unit-family <UNIT>   Unit family for rates and sizes [default: bin-bytes]
                             [possible values: bin-bytes, bin-bits, si-bytes, si-bits]
  -t, --total                Display cumulative totals instead of per-second rates
  -p, --processes            Show only the processes table
  -C, --connections          Show only the connections table
  -a, --addresses            Show only the remote addresses table
  -s, --sort <COLUMN>        Initial sort column [default: total]
                             [possible values: total, rx, tx, pid, name]
  -f, --filter <QUERY>       Filter processes, connections, or addresses by substring
      --no-color             Disable color styling
  -h, --help                 Print help
  -V, --version              Print version
```

### Keybindings

- `q` / `F10` / `Ctrl+C`: Quit
- `Tab` / `0`..`3`: Switch view (`All` / `Processes` / `Connections` / `Addresses`)
- `j` / `k` or `Up` / `Down`: Move selection
- `s`: Cycle sort column (`Total` / `Rx` / `Tx` / `PID` / `Name`)
- `r`: Reverse sort order
- `/`: Filter rows (`Enter` / `Esc` to finish)
- `Space` / `p`: Pause or resume display updates
- `t`: Toggle rates vs cumulative totals
- `u`: Cycle units (`BinBytes` / `BinBits` / `SiBytes` / `SiBits`)
- `n`: Toggle reverse DNS display
- `?` / `F1` / `Esc`: Toggle or close the help overlay
