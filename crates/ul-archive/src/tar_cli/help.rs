//! Textos fixos do GNU tar 1.35 (`--help`, `--usage`, `--version`), capturados do oráculo.

pub const HELP: &str = include_str!("help.txt");
pub const USAGE: &str = include_str!("usage.txt");
pub const VERSION: &str = "tar (GNU tar) 1.35
Copyright (C) 2023 Free Software Foundation, Inc.
License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>.
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.

Written by John Gilmore and Jay Fenlason.
";

/// `--show-defaults`.
pub const DEFAULTS: &str =
    "--format=gnu -f- -b20 --quoting-style=escape --rmt-command=/usr/sbin/rmt --rsh-command=/usr/bin/rsh\n";
